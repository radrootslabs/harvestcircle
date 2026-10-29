package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlin.time.Clock

/** Retains presentation input and reads native comparisons; never reconstructs saved execution grants. */
class CountPresenter(
    private val runtime: HarvestCircleCountRuntime?,
    private val stock: HarvestCircleStockRuntime?,
    private val handling: HarvestCircleHandlingRuntime?,
    private val references: HarvestCircleReferenceRuntime?,
    private val scope: CoroutineScope,
    private val operationIds: OperationIdSource,
    private val nowText: () -> String = { Clock.System.now().toString() },
    private val onFinished: () -> Unit = {},
) {
    private val mutableState = MutableStateFlow(CountViewState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var generation = 0L
    private var closed = false
    private var job: Job? = null

    fun attach(id: String) {
        if (closed || runtime == null || stock == null || handling == null || references == null || hub == id) return
        generation += 1
        job?.cancel()
        hub = id
        mutableState.value = CountViewState(available = true)
    }

    fun beginOpening() = begin(CountTarget.Opening)

    fun beginObservation(groupId: String) = begin(CountTarget.Observe(groupId))

    fun beginReview(countId: String) = begin(CountTarget.Review(countId))

    private fun begin(target: CountTarget) {
        if (!canRead() || mutableState.value.loading) return
        if (mutableState.value.draft != null || mutableState.value.submission != null) {
            resume()
            return
        }
        mutableState.value = CountViewState(available = true, open = true, target = target)
        loadContext(initial = true)
    }

    fun edit(transform: (CountFields) -> CountFields) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.submission != null) return
        val fields = transform(draft.fields).copy(confirmed = false)
        if (fields.bounded()) mutableState.value = current.copy(draft = draft.copy(fields = fields), message = null)
    }

    fun countedNow() = edit { it.copy(cutoffKind = HubOccurrenceKind.Known, cutoffStart = nowText(), cutoffEnd = "") }

    fun confirm(value: Boolean) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.loading || current.needsReview || current.submission != null) return
        if (draft.context is CountContext.Review && !draft.context.review.ready) return
        mutableState.value = current.copy(draft = draft.copy(fields = draft.fields.copy(confirmed = value)))
    }

    fun park() {
        val current = mutableState.value
        if (current.submission != null) return
        mutableState.value =
            current.copy(
                open = false,
                needsReview = current.draft != null,
                review = null,
                draft =
                    current.draft?.let {
                        it.copy(fields = it.fields.copy(confirmed = false))
                    },
            )
    }

    fun resume() {
        if (!canRead() || mutableState.value.target == null) return
        mutableState.value = mutableState.value.copy(open = true)
        if (mutableState.value.submission == null) reviewContext()
    }

    fun discard() {
        val current = mutableState.value
        if (current.loading || current.submission != null) return
        generation += 1
        job?.cancel()
        mutableState.value = CountViewState(available = current.available)
    }

    fun reviewContext() {
        val current = mutableState.value
        if (current.loading || current.submission != null || current.target == null) return
        mutableState.value =
            current.copy(
                needsReview = true,
                review = null,
                draft = current.draft?.let { it.copy(fields = it.fields.copy(confirmed = false)) },
            )
        loadContext(initial = current.draft == null)
    }

    private fun loadContext(initial: Boolean) {
        val target = mutableState.value.target ?: return
        read {
            val result =
                when (target) {
                    CountTarget.Opening -> {
                        val page = requireNotNull(stock).stock(HubStockQuery(limit = 1U))
                        CountContext.Opening(page.stockRevision, page.catalogRevision, referenceChoices(page.catalogRevision))
                    }
                    is CountTarget.Observe -> {
                        val group = requireNotNull(handling).group(target.groupId)
                        check(group.group.id == target.groupId)
                        CountContext.Observe(group, referenceChoices(group.catalogRevision))
                    }
                    is CountTarget.Review -> {
                        val review = requireNotNull(runtime).countReview(target.countId)
                        check(review.observation.id == target.countId)
                        CountContext.Review(review, referenceChoices(review.catalogRevision))
                    }
                }
            val update: (CountViewState) -> CountViewState = { current ->
                if (initial) {
                    current.copy(draft = CountDraft(result, CountFields(cutoffStart = nowText())), needsReview = false, review = null)
                } else {
                    current.copy(review = result)
                }
            }
            update
        }
    }

    fun acceptReview() {
        val current = mutableState.value
        val review = current.review ?: return
        val draft = current.draft ?: return
        if (current.loading || current.submission != null) return
        mutableState.value =
            current.copy(
                draft = draft.copy(context = review, fields = draft.fields.copy(confirmed = false)),
                needsReview = false,
                review = null,
                message = "Current native context reviewed. Your input is retained.",
            )
    }

    fun submit(snapshot: SnapshotRevision) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (!canRead() || current.loading || current.needsReview || current.submission != null || !draft.fields.confirmed) return
        if (draft.context is CountContext.Review && !draft.context.review.ready) return
        val request =
            try {
                draft.request()
            } catch (error: IllegalArgumentException) {
                mutableState.value =
                    current.copy(message = "Check whole counts, selected pack/location, identification and UTC count fields.")
                return
            }
        val attempt = CountAttempt(operationIds.next(), request)
        val token = generation
        mutableState.value = current.copy(submission = CountSubmission.Pending(attempt), message = null)
        job =
            scope.launch {
                try {
                    val context = RequestContext(attempt.operation, snapshot, 10_000UL)
                    val service = requireNotNull(runtime)
                    val result =
                        when (request) {
                            is CountRequest.Opening -> CountResult.Opened(service.recordOpening(context, request.input))
                            is CountRequest.Observe -> CountResult.Observed(service.observeCount(context, request.input))
                            is CountRequest.Post -> CountResult.Posted(service.postCount(context, request.input))
                        }
                    check(result.operation == attempt.operation)
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(submission = CountSubmission.Recorded(attempt, result))
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        val problem = (error as? ApplicationFailure)?.problem
                        val outcome =
                            if (problem != null && problem.code in COUNT_REJECTIONS) {
                                CountSubmission.Rejected(attempt, problem)
                            } else {
                                CountSubmission.Uncertain(
                                    attempt,
                                    problem?.safeMessage ?: "The original count operation is unconfirmed. Check its recorded result.",
                                )
                            }
                        mutableState.value =
                            mutableState.value.copy(submission = outcome, needsReview = problem?.code == ApplicationErrorCode.HubConflict)
                    }
                }
            }
    }

    fun checkOriginal() {
        val submitted = mutableState.value.submission ?: return
        if (submitted is CountSubmission.Pending || submitted is CountSubmission.Recorded) return
        read {
            val service = requireNotNull(runtime)
            val result =
                when (submitted.attempt.request) {
                    is CountRequest.Opening -> service.openingOperation(submitted.attempt.operation)?.let(CountResult::Opened)
                    is CountRequest.Observe -> service.countObservationOperation(submitted.attempt.operation)?.let(CountResult::Observed)
                    is CountRequest.Post -> service.countPostOperation(submitted.attempt.operation)?.let(CountResult::Posted)
                }
            if (result != null) check(result.operation == submitted.attempt.operation)
            val outcome =
                if (result != null) {
                    CountSubmission.Recorded(submitted.attempt, result)
                } else if (submitted is CountSubmission.Rejected) {
                    submitted
                } else {
                    CountSubmission.Uncertain(
                        submitted.attempt,
                        "No original is visible. Older restored history does not establish absence; retain this operation for review.",
                    )
                }
            val update: (CountViewState) -> CountViewState = { current -> current.copy(submission = outcome) }
            update
        }
    }

    fun reviewRejected() {
        val current = mutableState.value
        if (current.submission !is CountSubmission.Rejected) return
        mutableState.value =
            current.copy(
                submission = null,
                needsReview = true,
                draft = current.draft?.let { it.copy(fields = it.fields.copy(confirmed = false)) },
            )
        reviewContext()
    }

    fun reviewRecordedObservation() {
        val recorded = mutableState.value.submission as? CountSubmission.Recorded ?: return
        val observed = recorded.result as? CountResult.Observed ?: return
        mutableState.value = CountViewState(available = true, open = true, target = CountTarget.Review(observed.observation.id))
        loadContext(initial = true)
    }

    fun finishRecorded() {
        val current = mutableState.value
        if (current.submission !is CountSubmission.Recorded) return
        mutableState.value = CountViewState(available = current.available)
        onFinished()
    }

    private suspend fun referenceChoices(revision: ULong): List<HubReferenceRecord> {
        val result = mutableListOf<HubReferenceRecord>()
        var offset = 0U
        repeat(64) {
            val page =
                requireNotNull(
                    references,
                ).references(HubReferenceQuery(expectedCatalogRevision = revision, offset = offset, limit = 64U))
            check(page.catalogRevision == revision && page.records.size <= 64)
            result += page.records
            if (!page.more) return result
            check(page.records.isNotEmpty())
            offset += page.records.size.toUInt()
        }
        error("Reference choices exceed the bounded catalog")
    }

    private fun read(operation: suspend () -> ((CountViewState) -> CountViewState)) {
        val current = mutableState.value
        if (!canRead() || current.loading || current.submission is CountSubmission.Pending) return
        generation += 1
        val token = generation
        job?.cancel()
        mutableState.value = current.copy(loading = true, message = null)
        job =
            scope.launch {
                try {
                    val update = operation()
                    if (isCurrent(token)) mutableState.value = update(mutableState.value)
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(
                                needsReview = true,
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "Current count context is unavailable. Your input is retained.",
                            )
                    }
                } finally {
                    if (isCurrent(token)) mutableState.value = mutableState.value.copy(loading = false)
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
        mutableState.value = CountViewState()
    }
}

private val COUNT_REJECTIONS =
    setOf(
        ApplicationErrorCode.HubInvalidInput,
        ApplicationErrorCode.HubConflict,
        ApplicationErrorCode.HubCapacity,
        ApplicationErrorCode.HubPermissionDenied,
        ApplicationErrorCode.CredentialMissing,
        ApplicationErrorCode.KeyringUnavailable,
    )
