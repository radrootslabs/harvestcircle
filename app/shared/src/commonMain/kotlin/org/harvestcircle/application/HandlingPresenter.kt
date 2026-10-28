package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** UI coordination only. Native reads and transactions own stock, work and authority. */
class HandlingPresenter(
    private val runtime: HarvestCircleHandlingRuntime?,
    private val references: HarvestCircleReferenceRuntime?,
    private val scope: CoroutineScope,
    private val operationIds: OperationIdSource,
) {
    private val mutableState = MutableStateFlow(HandlingViewState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var closed = false
    private var generation = 0L
    private var job: Job? = null
    private var workQuery: HubHandlingQuery? = null
    private var discrepancyQuery: HubHandlingQuery? = null

    fun attach(hubId: String) {
        if (closed || runtime == null || references == null || hubId == hub) return
        generation += 1
        job?.cancel()
        hub = hubId
        workQuery = null
        discrepancyQuery = null
        mutableState.value = HandlingViewState(available = true)
        refresh()
    }

    fun chooseTab(tab: HandlingTab) {
        val current = mutableState.value
        if (current.loading || current.submission != null) return
        mutableState.value = current.copy(tab = tab, group = null, selectedWork = null, selectedDiscrepancy = null)
        refresh()
    }

    fun search(text: String) {
        if (text.length <= 1024) mutableState.value = mutableState.value.copy(queryText = text)
    }

    fun showClosed(include: Boolean) {
        if (mutableState.value.loading || mutableState.value.submission != null) return
        mutableState.value = mutableState.value.copy(openOnly = !include)
        refresh()
    }

    fun refresh() = loadPage(HubHandlingQuery(text = mutableState.value.queryText, openOnly = mutableState.value.openOnly))

    fun nextPage() {
        val current = mutableState.value
        val query =
            when (current.tab) {
                HandlingTab.Work ->
                    current.work?.takeIf { it.more }?.let {
                        workQuery?.copy(
                            expectedStockRevision = it.stockRevision,
                            expectedCatalogRevision = it.catalogRevision,
                            offset = it.offset + it.rows.size.toUInt(),
                        )
                    }
                HandlingTab.Discrepancies ->
                    current.discrepancies?.takeIf { it.more }?.let {
                        discrepancyQuery?.copy(
                            expectedStockRevision = it.stockRevision,
                            expectedCatalogRevision = it.catalogRevision,
                            offset = it.offset + it.rows.size.toUInt(),
                        )
                    }
            } ?: return
        loadPage(query)
    }

    private fun loadPage(query: HubHandlingQuery) {
        val current = mutableState.value
        if (!canRead() || current.loading || current.submission is HandlingSubmission.Pending) return
        val tab = current.tab
        mutableState.value =
            current.copy(
                work = if (tab == HandlingTab.Work) null else current.work,
                discrepancies = if (tab == HandlingTab.Discrepancies) null else current.discrepancies,
                group = null,
                selectedWork = null,
                selectedDiscrepancy = null,
            )
        read {
            val work = if (tab == HandlingTab.Work) requireNotNull(runtime).work(query) else null
            val issues = if (tab == HandlingTab.Discrepancies) requireNotNull(runtime).discrepancies(query) else null
            val versions =
                if (work != null) {
                    HandlingVersions(work.stockRevision, work.catalogRevision)
                } else {
                    HandlingVersions(requireNotNull(issues).stockRevision, issues.catalogRevision)
                }
            val choices = handlingReferenceChoices(requireNotNull(references), versions.catalog)
            val update: (HandlingViewState) -> HandlingViewState = { current ->
                if (work != null) workQuery = query else discrepancyQuery = query
                current.copy(
                    work = work ?: current.work,
                    discrepancies = issues ?: current.discrepancies,
                    references = choices,
                    needsReview = current.needsReview || current.draft?.let { it.versions != versions } == true,
                )
            }
            update
        }
    }

    fun inspectGroup(id: String) {
        val current = mutableState.value
        if (current.submission != null) return
        if (current.draft != null) {
            mutableState.value =
                current.copy(formOpen = true, message = "Finish or discard your retained draft before selecting another group.")
            return
        }
        read {
            val selected = requireNotNull(runtime).group(id)
            val choices = handlingReferenceChoices(requireNotNull(references), selected.catalogRevision)
            val update: (HandlingViewState) -> HandlingViewState = {
                it.copy(
                    group = selected,
                    selectedWork = null,
                    selectedDiscrepancy = null,
                    references = choices,
                )
            }
            update
        }
    }

    fun selectWork(row: HubWorkRow) {
        val current = mutableState.value
        if (current.loading || current.draft != null || current.submission != null) return
        mutableState.value = current.copy(selectedWork = row, selectedDiscrepancy = null, group = null)
    }

    fun selectDiscrepancy(row: HubDiscrepancyRow) {
        val current = mutableState.value
        if (current.loading || current.draft != null || current.submission != null) return
        mutableState.value = current.copy(selectedDiscrepancy = row, selectedWork = null, group = null)
    }

    fun closeRecord() {
        if (mutableState.value.loading || mutableState.value.submission != null) return
        mutableState.value = mutableState.value.copy(group = null, selectedWork = null, selectedDiscrepancy = null)
    }

    fun begin(action: HandlingAction) {
        val current = mutableState.value
        if (current.loading || current.submission != null) return
        if (current.draft != null) {
            mutableState.value =
                current.copy(formOpen = true, message = "Your retained handling draft is open. Finish or discard it before another action.")
            return
        }
        val draft =
            when (action) {
                HandlingAction.Move -> current.group?.let { HandlingDraft.Move(it) }
                HandlingAction.NewWork -> current.group?.let { HandlingDraft.NewWork(it) }
                HandlingAction.OpenDiscrepancy -> current.group?.let { HandlingDraft.OpenDiscrepancy(it) }
                HandlingAction.Replan ->
                    current.selectedWork?.let { row ->
                        current.work?.let {
                            HandlingDraft.Replan(row, HandlingVersions(it.stockRevision, it.catalogRevision))
                        }
                    }
                HandlingAction.ResolveDiscrepancy ->
                    current.selectedDiscrepancy?.let { row ->
                        current.discrepancies?.let {
                            HandlingDraft.ResolveDiscrepancy(row, HandlingVersions(it.stockRevision, it.catalogRevision))
                        }
                    }
            } ?: return
        mutableState.value = current.copy(draft = draft, formOpen = true, needsReview = false, review = null, message = null)
    }

    fun edit(fields: HandlingFields) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.loading || current.submission != null || !fields.bounded()) return
        mutableState.value = current.copy(draft = draft.edit(fields), message = null)
    }

    fun parkDraft() {
        if (mutableState.value.submission == null) mutableState.value = mutableState.value.copy(formOpen = false)
    }

    fun resumeDraft() {
        if (mutableState.value.draft != null) mutableState.value = mutableState.value.copy(formOpen = true)
    }

    fun discardDraft() {
        if (mutableState.value.loading || mutableState.value.submission != null) return
        mutableState.value = mutableState.value.copy(draft = null, formOpen = false, needsReview = false, review = null, message = null)
    }

    fun submit(snapshot: SnapshotRevision) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (!canRead() || current.loading || current.needsReview || current.submission != null) return
        val input =
            try {
                draft.input()
            } catch (_: IllegalArgumentException) {
                mutableState.value =
                    current.copy(
                        message = "Choose the required fields and enter a whole-pack count or an ISO timestamp with an explicit offset.",
                    )
                return
            }
        val attempt = HandlingAttempt(operationIds.next(), input)
        mutableState.value = current.copy(submission = HandlingSubmission.Pending(attempt), message = null)
        val token = generation
        job =
            scope.launch {
                try {
                    val result = requireNotNull(runtime).applyHandling(RequestContext(attempt.operation, snapshot, 10_000UL), input)
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(submission = HandlingSubmission.Recorded(attempt, result))
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        val problem = (error as? ApplicationFailure)?.problem
                        val result =
                            if (problem != null && problem.code in HANDLING_REJECTIONS) {
                                HandlingSubmission.Rejected(attempt, problem)
                            } else {
                                HandlingSubmission.Uncertain(
                                    attempt,
                                    problem?.safeMessage ?: "The handling outcome is unconfirmed. Check its original result.",
                                )
                            }
                        mutableState.value =
                            mutableState.value.copy(submission = result, needsReview = problem?.code == ApplicationErrorCode.HubConflict)
                    }
                }
            }
    }

    fun checkOriginal() {
        val submitted = mutableState.value.submission ?: return
        if (submitted is HandlingSubmission.Pending || submitted is HandlingSubmission.Recorded) return
        read {
            val original = requireNotNull(runtime).handlingOperation(submitted.attempt.operation)
            val result =
                when {
                    original != null -> HandlingSubmission.Recorded(submitted.attempt, original)
                    submitted is HandlingSubmission.Rejected -> submitted
                    else ->
                        HandlingSubmission.Uncertain(
                            submitted.attempt,
                            "No original result is visible. Older restored history cannot prove absence. Keep this operation for review; do not submit another ID.",
                        )
                }
            val update: (HandlingViewState) -> HandlingViewState = { it.copy(submission = result) }
            update
        }
    }

    fun reviewRejected() {
        if (mutableState.value.loading || mutableState.value.submission !is HandlingSubmission.Rejected) return
        mutableState.value =
            mutableState.value.copy(
                submission = null,
                message = "Your input is retained. Review the reported problem before submitting again.",
            )
    }

    fun reviewContext() {
        val draft = mutableState.value.draft ?: return
        if (mutableState.value.submission != null) return
        read {
            val fresh = refreshHandlingDraft(requireNotNull(runtime), draft)
            val choices = handlingReferenceChoices(requireNotNull(references), fresh.versions.catalog)
            val update: (HandlingViewState) -> HandlingViewState = { it.copy(review = HandlingContextReview(fresh, choices)) }
            update
        }
    }

    fun acceptReview() {
        val current = mutableState.value
        val review = current.review ?: return
        if (current.loading || current.submission != null) return
        mutableState.value =
            current.copy(
                draft = review.draft,
                references = review.references,
                review = null,
                needsReview = false,
                message = "Current scope reviewed. Your observations and selections are retained.",
            )
    }

    fun finishRecorded() {
        if (mutableState.value.submission !is HandlingSubmission.Recorded) return
        mutableState.value =
            mutableState.value.copy(
                submission = null,
                draft = null,
                formOpen = false,
                group = null,
                selectedWork = null,
                selectedDiscrepancy = null,
                review = null,
                needsReview = false,
                message = null,
            )
        refresh()
    }

    private fun read(block: suspend () -> (HandlingViewState) -> HandlingViewState) {
        val current = mutableState.value
        if (!canRead() || current.loading || current.submission is HandlingSubmission.Pending) return
        val token = generation
        mutableState.value = current.copy(loading = true, message = null)
        job =
            scope.launch {
                try {
                    val update = block()
                    if (isCurrent(token)) mutableState.value = update(mutableState.value).copy(loading = false)
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(
                                loading = false,
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "The current handling records could not be read. Your input is retained.",
                            )
                    }
                }
            }
    }

    private fun canRead() = !closed && hub != null && runtime != null && references != null

    private fun isCurrent(token: Long) = !closed && generation == token

    fun close() {
        closed = true
        generation += 1
        job?.cancel()
        hub = null
        workQuery = null
        discrepancyQuery = null
        mutableState.value = HandlingViewState()
    }
}

private val HANDLING_REJECTIONS =
    setOf(
        ApplicationErrorCode.HubInvalidInput,
        ApplicationErrorCode.HubConflict,
        ApplicationErrorCode.HubCapacity,
        ApplicationErrorCode.HubPermissionDenied,
        ApplicationErrorCode.CredentialMissing,
        ApplicationErrorCode.KeyringUnavailable,
    )
