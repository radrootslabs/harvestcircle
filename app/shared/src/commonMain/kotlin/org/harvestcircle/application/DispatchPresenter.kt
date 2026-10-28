package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** Client orchestration; native storage decides preparation and physical authority. */
class DispatchPresenter(
    private val runtime: HarvestCircleDispatchRuntime?,
    private val stock: HarvestCircleStockRuntime?,
    private val handling: HarvestCircleHandlingRuntime?,
    private val references: HarvestCircleReferenceRuntime?,
    private val scope: CoroutineScope,
    private val operationIds: OperationIdSource,
) {
    private val mutableState = MutableStateFlow(DispatchViewState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var closed = false
    private var generation = 0L
    private var job: Job? = null
    private var pageQuery = HubDispatchQuery()
    private var stockQuery = HubStockQuery()

    fun attach(id: String) {
        if (closed || runtime == null || stock == null || handling == null || references == null || hub == id) return
        generation += 1
        job?.cancel()
        hub = id
        mutableState.value = DispatchViewState(available = true)
        refresh()
    }

    fun search(text: String) {
        if (text.length <= 1024 && mutableState.value.submission == null) mutableState.value = mutableState.value.copy(queryText = text)
    }

    fun filter(value: HubDispatchFilter) {
        if (mutableState.value.loading || mutableState.value.submission != null) return
        mutableState.value = mutableState.value.copy(filter = value)
        refresh()
    }

    fun refresh() = loadPage(HubDispatchQuery(filter = mutableState.value.filter, text = mutableState.value.queryText))

    fun nextPage() {
        val page = mutableState.value.page?.takeIf { it.more } ?: return
        loadPage(
            pageQuery.copy(
                expectedStockRevision = page.stockRevision,
                expectedCatalogRevision = page.catalogRevision,
                offset = page.offset + page.rows.size.toUInt(),
            ),
        )
    }

    private fun loadPage(query: HubDispatchQuery) {
        read {
            val page = requireNotNull(runtime).dispatchPage(query)
            check(page.offset == query.offset && page.rows.size <= 64 && page.total <= 1024U)
            check(query.expectedStockRevision == null || query.expectedStockRevision == page.stockRevision)
            check(query.expectedCatalogRevision == null || query.expectedCatalogRevision == page.catalogRevision)
            pageQuery = query
            val update: (DispatchViewState) -> DispatchViewState = {
                it.copy(
                    page = page,
                    needsReview =
                        it.needsReview ||
                            it.draft?.versions?.let { pins ->
                                pins != HandlingVersions(page.stockRevision, page.catalogRevision)
                            } == true,
                )
            }
            update
        }
    }

    fun inspect(id: String) {
        if (mutableState.value.draft != null || mutableState.value.submission != null) {
            mutableState.value =
                mutableState.value.copy(message = "Finish or discard your retained identification draft before selecting another record.")
            return
        }
        read {
            val record = requireNotNull(runtime).dispatchRecord(id)
            val update: (DispatchViewState) -> DispatchViewState = { it.copy(selected = record, selectedVersionId = null) }
            update
        }
    }

    fun version(id: String) {
        val current = mutableState.value
        if (current.loading ||
            current.submission != null ||
            current.selected
                ?.record
                ?.versions
                ?.none { it.id == id } != false
        ) {
            return
        }
        mutableState.value = current.copy(selectedVersionId = id)
    }

    fun closeRecord() {
        if (!mutableState.value.loading &&
            mutableState.value.submission == null
        ) {
            mutableState.value = mutableState.value.copy(selected = null)
        }
    }

    fun begin(kind: HubDispatchKind) {
        val current = mutableState.value
        if (current.loading || current.submission != null) return
        if (current.draft != null) {
            resume()
            return
        }
        val id = if (kind == HubDispatchKind.Prepare) null else current.selected?.record?.id
        if (kind != HubDispatchKind.Prepare && id == null) return
        read {
            val page = requireNotNull(stock).stock(HubStockQuery())
            val target = id?.let { requireNotNull(runtime).dispatchRecord(it) }
            if (target != null) check(target.stockRevision == page.stockRevision && target.catalogRevision == page.catalogRevision)
            val choices = handlingReferenceChoices(requireNotNull(references), page.catalogRevision)
            val material = target?.record?.material()
            val draft =
                DispatchDraft(
                    kind,
                    HandlingVersions(page.stockRevision, page.catalogRevision),
                    target?.record,
                    material?.recipient?.id,
                    material?.lines.orEmpty(),
                )
            stockQuery = HubStockQuery()
            val update: (DispatchViewState) -> DispatchViewState = {
                it.copy(
                    draft = draft,
                    stock = page,
                    references = choices,
                    formOpen = true,
                    review = null,
                    needsReview = kind == HubDispatchKind.Revise || kind == HubDispatchKind.Replace,
                )
            }
            update
        }
    }

    fun edit(
        recipient: String?,
        observation: String,
    ) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.submission != null || observation.length > 1024) return
        mutableState.value = current.copy(draft = draft.copy(recipientId = recipient, observation = observation), review = null)
    }

    fun removeGroup(id: String) {
        val current = mutableState.value
        if (current.loading || current.submission != null) return
        mutableState.value =
            current.copy(draft = current.draft?.let { it.copy(lines = it.lines.filterNot { line -> line.groupId == id }) }, review = null)
    }

    fun selectGroup(id: String) {
        val draft = mutableState.value.draft ?: return
        if (mutableState.value.submission != null || draft.lines.size >= 64 || draft.lines.any { it.groupId == id }) return
        read {
            val selected = requireNotNull(handling).group(id)
            check(selected.stockRevision == draft.versions.stock && selected.catalogRevision == draft.versions.catalog)
            val update: (DispatchViewState) -> DispatchViewState = {
                it.copy(draft = it.draft?.let { latest -> latest.copy(lines = latest.lines + selected.dispatchLine()) }, review = null)
            }
            update
        }
    }

    fun nextStockPage() {
        val current = mutableState.value.stock?.takeIf { it.more } ?: return
        if (mutableState.value.submission != null) return
        val query =
            stockQuery.copy(
                expectedStockRevision = current.stockRevision,
                expectedCatalogRevision = current.catalogRevision,
                offset = current.offset + current.rows.size.toUInt(),
            )
        read {
            val next = requireNotNull(stock).stock(query)
            check(
                next.stockRevision == current.stockRevision &&
                    next.catalogRevision == current.catalogRevision &&
                    next.offset == query.offset &&
                    next.rows.size <= 64,
            )
            stockQuery = query
            val update: (DispatchViewState) -> DispatchViewState = { it.copy(stock = next) }
            update
        }
    }

    fun park() {
        if (mutableState.value.submission == null) mutableState.value = mutableState.value.copy(formOpen = false)
    }

    fun resume() {
        if (mutableState.value.draft != null) mutableState.value = mutableState.value.copy(formOpen = true)
    }

    fun discard() {
        if (mutableState.value.loading || mutableState.value.submission != null) return
        mutableState.value = mutableState.value.copy(draft = null, formOpen = false, needsReview = false, review = null, message = null)
    }

    fun reviewContext() {
        val draft = mutableState.value.draft ?: return
        if (mutableState.value.submission != null || mutableState.value.loading) return
        mutableState.value = mutableState.value.copy(needsReview = true, review = null)
        read {
            val page = requireNotNull(stock).stock(HubStockQuery())
            val target = draft.target?.id?.let { requireNotNull(runtime).dispatchRecord(it) }
            if (target != null) check(target.stockRevision == page.stockRevision && target.catalogRevision == page.catalogRevision)
            val captured =
                if (draft.kind == HubDispatchKind.Issue) {
                    target
                        ?.record
                        ?.material()
                        ?.lines
                        .orEmpty()
                } else {
                    draft.lines
                }
            val lines =
                if (draft.kind == HubDispatchKind.MarkUnused) {
                    target
                        ?.record
                        ?.material()
                        ?.lines
                        .orEmpty()
                } else {
                    captured.map { old ->
                        val group = requireNotNull(handling).group(old.groupId)
                        check(group.stockRevision == page.stockRevision && group.catalogRevision == page.catalogRevision)
                        group.dispatchLine()
                    }
                }
            val choices = handlingReferenceChoices(requireNotNull(references), page.catalogRevision)
            val fresh =
                draft.copy(
                    versions = HandlingVersions(page.stockRevision, page.catalogRevision),
                    target = target?.record,
                    lines = lines,
                )
            val update: (DispatchViewState) -> DispatchViewState = {
                it.copy(
                    review =
                        DispatchContextReview(
                            fresh.copy(
                                recipientId = it.draft?.recipientId,
                                observation = it.draft?.observation.orEmpty(),
                            ),
                            choices,
                            page,
                        ),
                )
            }
            update
        }
    }

    fun acceptReview() {
        val current = mutableState.value
        val review = current.review ?: return
        if (current.loading || current.submission != null) return
        stockQuery = HubStockQuery()
        mutableState.value =
            current.copy(
                draft = review.draft,
                references = review.references,
                stock = review.stock,
                needsReview = false,
                review = null,
                message = "Current material and target reviewed. Your recipient choice and observations are retained.",
            )
    }

    fun submit(snapshot: SnapshotRevision) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (!canRead() || current.loading || current.needsReview || current.submission != null) return
        val input =
            try {
                draft.input()
            } catch (_: IllegalArgumentException) {
                mutableState.value = current.copy(message = "Choose the recipient and whole source groups, then describe your observation.")
                return
            }
        val attempt = DispatchAttempt(operationIds.next(), input)
        mutableState.value = current.copy(submission = DispatchSubmission.Pending(attempt), message = null)
        val token = generation
        job =
            scope.launch {
                try {
                    val receipt = requireNotNull(runtime).applyDispatch(RequestContext(attempt.operation, snapshot, 10_000UL), input)
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(submission = DispatchSubmission.Recorded(attempt, receipt))
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        val problem = (error as? ApplicationFailure)?.problem
                        val result =
                            if (problem != null && problem.code in DISPATCH_REJECTIONS) {
                                DispatchSubmission.Rejected(attempt, problem)
                            } else {
                                DispatchSubmission.Uncertain(
                                    attempt,
                                    problem?.safeMessage ?: "The identification outcome is unconfirmed. Check its original result.",
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
        if (submitted is DispatchSubmission.Pending || submitted is DispatchSubmission.Recorded) return
        read {
            val original = requireNotNull(runtime).dispatchOperation(submitted.attempt.operation)
            val result =
                when {
                    original != null -> DispatchSubmission.Recorded(submitted.attempt, original)
                    submitted is DispatchSubmission.Rejected -> submitted
                    else ->
                        DispatchSubmission.Uncertain(
                            submitted.attempt,
                            "No original result is visible. Restored older history cannot prove absence. Keep this operation for review; do not submit another ID.",
                        )
                }
            val update: (DispatchViewState) -> DispatchViewState = { it.copy(submission = result) }
            update
        }
    }

    fun reviewRejected() {
        if (!mutableState.value.loading && mutableState.value.submission is DispatchSubmission.Rejected) {
            mutableState.value =
                mutableState.value.copy(
                    submission = null,
                    message = "Your input is retained. Review the native problem before submitting again.",
                )
        }
    }

    fun finishRecorded() {
        val result = mutableState.value.submission as? DispatchSubmission.Recorded ?: return
        mutableState.value =
            mutableState.value.copy(
                submission = null,
                draft = null,
                formOpen = false,
                needsReview = false,
                review = null,
                selected = null,
                selectedVersionId = null,
                message = null,
            )
        inspect(result.receipt.record.id)
    }

    private fun read(block: suspend () -> (DispatchViewState) -> DispatchViewState) {
        val current = mutableState.value
        if (!canRead() || current.loading || current.submission is DispatchSubmission.Pending) return
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
                                needsReview = mutableState.value.draft != null || mutableState.value.needsReview,
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "Current identification records could not be read. Your input is retained.",
                            )
                    }
                }
            }
    }

    private fun canRead() = !closed && hub != null && runtime != null && stock != null && handling != null && references != null

    private fun isCurrent(token: Long) = !closed && generation == token

    fun close() {
        closed = true
        generation += 1
        job?.cancel()
        hub = null
        mutableState.value = DispatchViewState()
    }
}

private val DISPATCH_REJECTIONS =
    setOf(
        ApplicationErrorCode.HubInvalidInput,
        ApplicationErrorCode.HubConflict,
        ApplicationErrorCode.HubCapacity,
        ApplicationErrorCode.HubPermissionDenied,
        ApplicationErrorCode.CredentialMissing,
        ApplicationErrorCode.KeyringUnavailable,
    )
