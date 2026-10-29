package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** Retained client orchestration. Rust alone decides readiness and physical effects. */
class DeparturePresenter(
    private val runtime: HarvestCircleDepartureRuntime?,
    private val scope: CoroutineScope,
    private val operationIds: OperationIdSource,
) {
    private val mutableState = MutableStateFlow(DepartureViewState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var closed = false
    private var generation = 0L
    private var job: Job? = null

    fun attach(id: String) {
        if (closed || runtime == null || hub == id) return
        generation += 1
        job?.cancel()
        hub = id
        mutableState.value = DepartureViewState(available = true)
    }

    fun begin(view: HubDispatchView) {
        val current = mutableState.value
        if (!canRead() || current.loading || current.submission != null) return
        if (current.draft != null) {
            resume()
            return
        }
        val version = view.record.versions.lastOrNull()
        if (view.state != HubDispatchState.Issued || version == null) return
        mutableState.value =
            current.copy(
                formOpen = true,
                draft = DepartureDraft(view.record, HubDepartureReviewInput(view.record.id, view.record.revision, version.id)),
                message = null,
            )
        loadReview(initial = true)
    }

    fun edit(fields: HandlingFields) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.submission != null || !fields.bounded()) return
        mutableState.value = current.copy(draft = draft.copy(fields = fields, confirmed = false))
    }

    fun confirm(value: Boolean) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.loading || current.needsReview || current.submission != null || draft.context?.ready != true) return
        mutableState.value = current.copy(draft = draft.copy(confirmed = value))
    }

    fun park() {
        val current = mutableState.value
        if (current.submission != null) return
        mutableState.value =
            current.copy(
                formOpen = false,
                needsReview = current.draft != null,
                draft = current.draft?.copy(confirmed = false),
                review = null,
            )
    }

    fun resume() {
        if (mutableState.value.draft == null && mutableState.value.submission == null) return
        mutableState.value = mutableState.value.copy(formOpen = true)
        if (mutableState.value.submission == null) reviewContext()
    }

    fun discard() {
        val current = mutableState.value
        if (current.loading || current.submission != null) return
        mutableState.value = DepartureViewState(available = current.available)
    }

    fun reviewContext() {
        val current = mutableState.value
        if (current.draft == null || current.loading || current.submission != null) return
        mutableState.value = current.copy(needsReview = true, draft = current.draft.copy(confirmed = false), review = null)
        loadReview(initial = false)
    }

    private fun loadReview(initial: Boolean) {
        val current = mutableState.value
        val request = current.draft?.request ?: return
        if (!canRead() || current.loading || current.submission != null) return
        val token = generation
        mutableState.value = current.copy(loading = true, message = null)
        job =
            scope.launch {
                try {
                    val result = requireNotNull(runtime).departureReview(request)
                    check(result.request == request && result.view.record.id == request.preparationId)
                    if (isCurrent(token)) {
                        val latest = mutableState.value
                        val draft = latest.draft?.takeIf { it.request == request } ?: return@launch
                        mutableState.value =
                            if (initial) {
                                latest.copy(loading = false, draft = draft.copy(context = result, confirmed = false), workOffset = 0)
                            } else {
                                latest.copy(loading = false, review = result, workOffset = 0)
                            }
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(
                                loading = false,
                                needsReview = true,
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "Current departure context could not be read. Your input is retained.",
                            )
                    }
                }
            }
    }

    fun acceptReview() {
        val current = mutableState.value
        val review = current.review ?: return
        val draft = current.draft ?: return
        if (current.loading || current.submission != null || !review.ready || review.request != draft.request) return
        mutableState.value =
            current.copy(
                draft = draft.copy(context = review, confirmed = false),
                review = null,
                needsReview = false,
                message = "Current stock and the same issued identification reviewed. Your observations are retained.",
                workOffset = 0,
            )
    }

    fun workPage(delta: Int) {
        val current = mutableState.value
        val count =
            (current.submission as? DepartureSubmission.Recorded)?.receipt?.workChanges?.size
                ?: (current.review ?: current.draft?.context)?.proposedWorkChanges?.size ?: 0
        val offset = current.workOffset + delta * 64
        if (delta in -1..1 && offset >= 0 && offset < count) mutableState.value = current.copy(workOffset = offset)
    }

    fun submit(snapshot: SnapshotRevision) {
        val current = mutableState.value
        val draft = current.draft ?: return
        val context = draft.context ?: return
        if (!canRead() || current.loading || current.needsReview || current.submission != null) return
        if (!draft.confirmed || !context.ready) return
        val input =
            try {
                require(draft.fields.observation.isNotBlank())
                HubDepartureInput(
                    context.view.stockRevision,
                    context.view.catalogRevision,
                    draft.request.preparationId,
                    draft.request.expectedPreparationRevision,
                    draft.request.identificationId,
                    draft.fields.occurrence(),
                    draft.fields.observation,
                )
            } catch (_: IllegalArgumentException) {
                mutableState.value = current.copy(message = "Describe the actual departure and review the occurrence time.")
                return
            }
        val attempt = DepartureAttempt(operationIds.next(), input)
        val token = generation
        mutableState.value = current.copy(submission = DepartureSubmission.Pending(attempt), message = null)
        job =
            scope.launch {
                try {
                    val receipt = requireNotNull(runtime).applyDeparture(RequestContext(attempt.operation, snapshot, 10_000UL), input)
                    checkReceipt(attempt, receipt)
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(submission = DepartureSubmission.Recorded(attempt, receipt), workOffset = 0)
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        val problem = (error as? ApplicationFailure)?.problem
                        val result =
                            if (problem != null && problem.code in DEPARTURE_REJECTIONS) {
                                DepartureSubmission.Rejected(attempt, problem)
                            } else {
                                DepartureSubmission.Uncertain(
                                    attempt,
                                    problem?.safeMessage ?: "The departure outcome is unconfirmed. Check this operation's original result.",
                                )
                            }
                        mutableState.value = mutableState.value.copy(submission = result, needsReview = true)
                    }
                }
            }
    }

    fun checkOriginal() {
        val current = mutableState.value
        val submitted = current.submission ?: return
        if (current.loading || submitted is DepartureSubmission.Pending || submitted is DepartureSubmission.Recorded) return
        val token = generation
        mutableState.value = current.copy(loading = true, message = null)
        job =
            scope.launch {
                try {
                    val original = requireNotNull(runtime).departureOperation(submitted.attempt.operation)
                    original?.let { checkReceipt(submitted.attempt, it) }
                    if (isCurrent(token)) {
                        val result =
                            when {
                                original != null -> DepartureSubmission.Recorded(submitted.attempt, original)
                                submitted is DepartureSubmission.Rejected -> submitted
                                else ->
                                    DepartureSubmission.Uncertain(
                                        submitted.attempt,
                                        "No original result is visible. Older restored history cannot prove absence. Keep this operation for review; do not submit another ID.",
                                    )
                            }
                        mutableState.value = mutableState.value.copy(loading = false, submission = result, workOffset = 0)
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(
                                loading = false,
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "The original departure could not be read. Keep this operation for review.",
                            )
                    }
                }
            }
    }

    fun reviewRejected() {
        val current = mutableState.value
        if (current.loading || current.submission !is DepartureSubmission.Rejected) return
        mutableState.value = current.copy(submission = null, needsReview = true, draft = current.draft?.copy(confirmed = false))
        reviewContext()
    }

    fun finishRecorded(): String? {
        val result = mutableState.value.submission as? DepartureSubmission.Recorded ?: return null
        val id = result.receipt.paper.id
        mutableState.value = DepartureViewState(available = mutableState.value.available)
        return id
    }

    private fun checkReceipt(
        attempt: DepartureAttempt,
        receipt: HubDepartureReceipt,
    ) {
        check(
            receipt.operationId == attempt.operation &&
                receipt.paper.id == attempt.input.preparationId &&
                receipt.paper.revision == attempt.input.expectedPreparationRevision,
        )
        check(
            receipt.paper.versions
                .lastOrNull()
                ?.id == attempt.input.identificationId,
        )
        check(receipt.catalogRevision == attempt.input.expectedCatalogRevision)
        check(receipt.occurrence == attempt.input.occurrence && receipt.observation == attempt.input.observation)
    }

    private fun canRead() = !closed && hub != null && runtime != null

    private fun isCurrent(token: Long) = !closed && token == generation

    fun close() {
        closed = true
        generation += 1
        job?.cancel()
        hub = null
        mutableState.value = DepartureViewState()
    }
}

private val DEPARTURE_REJECTIONS =
    setOf(
        ApplicationErrorCode.HubInvalidInput,
        ApplicationErrorCode.HubConflict,
        ApplicationErrorCode.HubCapacity,
        ApplicationErrorCode.HubPermissionDenied,
        ApplicationErrorCode.CredentialMissing,
        ApplicationErrorCode.KeyringUnavailable,
    )
