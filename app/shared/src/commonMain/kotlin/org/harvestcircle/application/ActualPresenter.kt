package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** Owns in-memory input and exact operation recovery; native code owns every effect. */
class ActualPresenter(
    private val runtime: HarvestCircleActualRuntime?,
    private val stock: HarvestCircleStockRuntime?,
    private val handling: HarvestCircleHandlingRuntime?,
    private val references: HarvestCircleReferenceRuntime?,
    private val traces: HarvestCircleTraceRuntime?,
    private val scope: CoroutineScope,
    private val operationIds: OperationIdSource,
    private val onFinished: () -> Unit = {},
) {
    private val mutableState = MutableStateFlow(ActualViewState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var binding: NativeSessionBinding? = null
    private var generation = 0L
    private var closed = false
    private var job: Job? = null

    fun attach(
        id: String,
        native: NativeSessionBinding,
    ) {
        if (closed || runtime == null) return
        if (hub != null) {
            if (hub != id || binding != native) close()
            return
        }
        hub = id
        binding = native
        mutableState.value = ActualViewState(available = true)
    }

    fun open() {
        if (!canRead()) return
        mutableState.value = mutableState.value.copy(open = true)
        if (mutableState.value.draft == null && mutableState.value.submission == null) refreshReports()
    }

    fun refreshReports(
        filter: HubActualReportFilter = mutableState.value.filter,
        next: Boolean = false,
    ) {
        val current = mutableState.value
        if (current.draft != null || current.submission != null || current.originalLookup != null) return
        val previous = current.page?.takeIf { next && it.more && filter == current.filter }
        if (next && previous == null) return
        read {
            val page =
                requireNotNull(runtime).actualReports(
                    HubActualReportQuery(
                        expectedStockRevision = previous?.stockRevision,
                        expectedCatalogRevision = previous?.catalogRevision,
                        offset = previous?.let { it.offset + it.rows.size.toUInt() } ?: 0U,
                        filter = filter,
                    ),
                )
            val update: (ActualViewState) -> ActualViewState = { it.copy(page = page, filter = filter, open = true) }
            update
        }
    }

    fun beginReport() = begin(ActualTarget.Report)

    fun beginDeparture(report: HubActualReport) = begin(ActualTarget.Departure(report))

    fun beginLink(report: HubActualReport) = begin(ActualTarget.Link(report))

    private fun begin(target: ActualTarget) {
        val current = mutableState.value
        if (!canRead() || current.loading || current.submission != null || current.draft != null || current.originalLookup != null) return
        val report =
            when (target) {
                ActualTarget.Report -> null
                is ActualTarget.Departure -> target.report
                is ActualTarget.Link -> target.report
            }
        check(report == null || report.hubId == hub)
        mutableState.value =
            ActualViewState(
                available = true,
                open = true,
                draft =
                    ActualDraft(
                        target,
                        fields =
                            report?.let(ActualFields::from) ?: ActualFields(),
                    ),
            )
        // Saving an unknown report requires no positive stock or successful context read.
    }

    fun edit(transform: (ActualFields) -> ActualFields) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.loading || current.submission != null) return
        val fields = transform(draft.fields).copy(confirmed = false)
        if (!fields.bounded()) return
        val selectionChanged = fields.groupId != draft.fields.groupId || fields.traceId != draft.fields.traceId
        mutableState.value =
            current.copy(
                draft =
                    draft.copy(
                        fields =
                            fields.copy(
                                source = fields.source.takeUnless { selectionChanged },
                                known = fields.known.takeUnless { selectionChanged },
                            ),
                        context =
                            if (selectionChanged) {
                                draft.context.copy(
                                    group = null,
                                    sources = emptyList(),
                                    known = emptyList(),
                                    sourceHistory = null,
                                    lineage = null,
                                )
                            } else {
                                draft.context
                            },
                    ),
                currentContext = null,
                preview = null,
                needsReview =
                    current.needsReview || (selectionChanged && draft.target != ActualTarget.Report),
                message = null,
            )
    }

    fun refreshContext() {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.submission != null || current.loading) return
        mutableState.value =
            current.copy(
                currentContext = null,
                preview = null,
                needsReview = draft.target != ActualTarget.Report,
                draft = draft.unconfirmed(),
            )
        read {
            val context = ActualContextReader(stock, handling, references, traces).load(draft.fields)
            val update: (ActualViewState) -> ActualViewState = { it.copy(currentContext = context) }
            update
        }
    }

    fun acceptContext() {
        val current = mutableState.value
        val draft = current.draft ?: return
        val context = current.currentContext ?: return
        if (current.loading || current.submission != null) return
        mutableState.value =
            current.copy(
                draft =
                    draft.unconfirmed().copy(
                        context = context,
                        fields = draft.fields.copy(source = null, known = null, confirmed = false),
                    ),
                currentContext = null,
                needsReview = false,
                preview = null,
            )
    }

    fun nextChoices(kind: ActualChoicePage) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.submission != null || current.needsReview || current.currentContext != null) return
        read {
            val context = ActualContextReader(stock, handling, references, traces).next(draft.context, kind)
            val update: (ActualViewState) -> ActualViewState = { it.copy(draft = requireNotNull(it.draft).copy(context = context)) }
            update
        }
    }

    fun preview() {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.loading || current.submission != null || current.needsReview || draft.target == ActualTarget.Report) return
        val request = request(draft) ?: return
        val attempt = ActualAttempt(operationIds.next(), request)
        mutableState.value = current.copy(preview = null, draft = draft.unconfirmed())
        read {
            val service = requireNotNull(runtime)
            val preview =
                when (request) {
                    is ActualRequest.Departure ->
                        ActualPreview.Departure(
                            attempt,
                            service.actualDeparturePreview(attempt.operation, request.input),
                        )
                    is ActualRequest.Link -> ActualPreview.Link(attempt, service.actualLinkPreview(attempt.operation, request.input))
                    is ActualRequest.Report -> error("Report ingress has no physical preview")
                }
            when (preview) {
                is ActualPreview.Departure -> {
                    val input = (request as ActualRequest.Departure).input
                    check(preview.value.stockRevision == input.expectedStockRevision)
                    check(preview.value.catalogRevision == input.expectedCatalogRevision)
                    check(preview.value.report.id == input.reportId)
                    verifyDeparture(attempt.operation, input, preview.value.proposed)
                }
                is ActualPreview.Link -> {
                    verifyLink(attempt.operation, (request as ActualRequest.Link).input, preview.value.proposed)
                }
            }
            val update: (ActualViewState) -> ActualViewState = { it.copy(preview = preview) }
            update
        }
    }

    fun confirm(value: Boolean) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.loading || current.submission != null || current.needsReview) return
        val request = request(draft) ?: return
        if (request !is ActualRequest.Report && current.preview?.attempt?.request != request) return
        mutableState.value = current.copy(draft = draft.copy(fields = draft.fields.copy(confirmed = value)))
    }

    fun submit(snapshot: SnapshotRevision) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (!canRead() || current.loading || current.submission != null || current.needsReview || !draft.fields.confirmed) return
        val request = request(draft) ?: return
        val attempt =
            if (request is ActualRequest.Report) {
                ActualAttempt(operationIds.next(), request)
            } else {
                current.preview?.attempt?.takeIf {
                    it.request ==
                        request
                }
                    ?: return
            }
        val token = generation
        mutableState.value = current.copy(submission = ActualSubmission.Pending(attempt), message = null)
        job =
            scope.launch {
                try {
                    val service = requireNotNull(runtime)
                    val context = RequestContext(attempt.operation, snapshot, 10_000UL)
                    val result =
                        when (request) {
                            is ActualRequest.Report -> ActualResult.Reported(service.reportActual(context, request.input))
                            is ActualRequest.Departure -> ActualResult.Reviewed(service.reconcileActual(context, request.input))
                            is ActualRequest.Link -> ActualResult.Reviewed(service.linkActual(context, request.input))
                        }
                    verify(attempt, result)
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(submission = ActualSubmission.Recorded(attempt, result))
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        val problem = (error as? ApplicationFailure)?.problem
                        val outcome =
                            if (problem != null &&
                                problem.code in ACTUAL_REJECTIONS
                            ) {
                                ActualSubmission.Rejected(attempt, problem)
                            } else {
                                ActualSubmission.Uncertain(
                                    attempt,
                                    problem?.safeMessage ?: "Original result unconfirmed. Retain this operation and read its original.",
                                )
                            }
                        mutableState.value = mutableState.value.copy(submission = outcome)
                    }
                }
            }
    }

    fun checkOriginal() {
        val submitted = mutableState.value.submission ?: return
        if (submitted is ActualSubmission.Pending || submitted is ActualSubmission.Recorded) return
        read {
            val result = original(submitted.attempt.operation, submitted.attempt.request.lookupKind())
            if (result != null) verify(submitted.attempt, result)
            val outcome =
                when {
                    result != null -> ActualSubmission.Recorded(submitted.attempt, result)
                    submitted is ActualSubmission.Rejected -> submitted
                    else ->
                        ActualSubmission.Uncertain(
                            submitted.attempt,
                            "No original is visible. Absence after restore does not prove no effect or authorize another operation.",
                        )
                }
            val update: (ActualViewState) -> ActualViewState = { it.copy(submission = outcome) }
            update
        }
    }

    fun reviewRejected() {
        val current = mutableState.value
        if (current.submission !is ActualSubmission.Rejected) return
        mutableState.value =
            current.copy(
                submission = null,
                preview = null,
                currentContext = null,
                draft = current.draft?.unconfirmed(),
                needsReview =
                    current.draft?.target != ActualTarget.Report,
            )
        if (current.draft?.target != ActualTarget.Report) refreshContext()
    }

    fun beginOriginalLookup() {
        val current = mutableState.value
        if (!canRead() || current.loading || current.draft != null || current.submission != null) return
        mutableState.value = current.copy(open = true, originalLookup = ActualOriginalLookup())
    }

    fun editOriginalLookup(
        kind: ActualLookupKind,
        text: String,
    ) {
        val current = mutableState.value
        if (current.originalLookup == null || current.loading || text.length > 36) return
        mutableState.value = current.copy(originalLookup = ActualOriginalLookup(kind, text), message = null)
    }

    fun lookupOriginal() {
        val lookup = mutableState.value.originalLookup ?: return
        val operation =
            try {
                OperationId.from(lookup.operationText)
            } catch (_: IllegalArgumentException) {
                mutableState.value = mutableState.value.copy(message = "Enter the complete canonical original operation ID.")
                return
            }
        read {
            val result = original(operation, lookup.kind)
            val update: (ActualViewState) -> ActualViewState = {
                it.copy(
                    originalLookup = lookup.copy(result = result),
                    message =
                        if (result ==
                            null
                        ) {
                            "No original visible. Absence grants no resubmission permission."
                        } else {
                            "Immutable original read. Historical facts grant no current permission."
                        },
                )
            }
            update
        }
    }

    fun finishRecorded() {
        if (mutableState.value.submission !is ActualSubmission.Recorded) return
        mutableState.value = ActualViewState(available = true, open = true)
        onFinished()
        refreshReports()
    }

    fun park() {
        val current = mutableState.value
        if (current.submission != null) return
        generation++
        job?.cancel()
        mutableState.value =
            current.copy(
                open = false,
                loading = false,
                draft = current.draft?.unconfirmed(),
                preview = null,
                currentContext = null,
                needsReview =
                    current.draft?.target != null && current.draft.target != ActualTarget.Report,
            )
    }

    fun resume() {
        if (!canRead() || mutableState.value.loading) return
        mutableState.value = mutableState.value.copy(open = true)
        if (mutableState.value.needsReview && mutableState.value.submission == null) refreshContext()
    }

    fun discard() {
        if (mutableState.value.loading || mutableState.value.submission != null) return
        generation++
        job?.cancel()
        mutableState.value = ActualViewState(available = true, open = true)
        refreshReports()
    }

    fun close() {
        closed = true
        generation++
        job?.cancel()
        hub = null
        binding = null
        mutableState.value = ActualViewState()
    }

    private fun request(draft: ActualDraft): ActualRequest? =
        try {
            draft.request()
        } catch (_: IllegalArgumentException) {
            mutableState.value =
                mutableState.value.copy(
                    message = "Review whole packs, source, recipient, UTC occurrence and explicit verification evidence.",
                )
            null
        }

    private suspend fun original(
        operation: OperationId,
        kind: ActualLookupKind,
    ): ActualResult? {
        val service = requireNotNull(runtime)
        val result =
            when (kind) {
                ActualLookupKind.Report -> service.actualReportOperation(operation)?.let(ActualResult::Reported)
                ActualLookupKind.Review -> service.actualReviewOperation(operation)?.let(ActualResult::Reviewed)
            }
        check(result == null || result.operation == operation)
        return result
    }

    private fun verify(
        attempt: ActualAttempt,
        result: ActualResult,
    ) {
        check(result.operation == attempt.operation)
        when (val request = attempt.request) {
            is ActualRequest.Report -> {
                val report = (result as ActualResult.Reported).value
                check(report.reported == request.input && report.hubId == hub)
            }
            is ActualRequest.Departure -> {
                val original = ((result as ActualResult.Reviewed).value.original as HubActualReviewOriginal.Departure)
                check(original.report.id == request.input.reportId && original.report.hubId == hub)
                verifyDeparture(attempt.operation, request.input, original.original)
            }
            is ActualRequest.Link -> {
                val original = ((result as ActualResult.Reviewed).value.original as HubActualReviewOriginal.Duplicate).original
                verifyLink(attempt.operation, request.input, original)
            }
        }
    }

    private fun verifyDeparture(
        operation: OperationId,
        input: HubActualDepartureInput,
        original: HubTraceActualDeparture,
    ) {
        check(original.operationId == operation && original.reportId == input.reportId)
        check(original.before.id == input.groupId && original.before.revision == input.expectedGroupRevision)
        check(original.wholePacks == input.wholePacks && original.recipient.id == input.recipientId)
        check(original.occurrence == input.occurrence)
        check(original.sourceOperationId == input.sourceOperationId && original.sourceGroupId == input.sourceGroupId)
        check(original.sourceEvidence == input.sourceEvidence && original.quantityEvidence == input.quantityEvidence)
        check(original.recipientEvidence == input.recipientEvidence && original.timingEvidence == input.timingEvidence)
        check(original.duplicateReviewBasis == input.duplicateReviewBasis)
    }

    private fun verifyLink(
        operation: OperationId,
        input: HubActualLinkInput,
        original: HubActualDuplicate,
    ) {
        check(original.operationId == operation && original.report.id == input.reportId && original.report.hubId == hub)
        check(original.known.operationId == input.knownOperationId && original.known.traceId == input.knownTraceId)
        check(original.known.variant.id == input.knownVariantId && original.verification == input.verification)
    }

    private fun read(operation: suspend () -> ((ActualViewState) -> ActualViewState)) {
        val current = mutableState.value
        if (!canRead() || current.loading || current.submission is ActualSubmission.Pending) return
        generation++
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
                                currentContext = null,
                                preview = null,
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "Read unavailable. Input and original operation retained.",
                            )
                    }
                } finally {
                    if (isCurrent(token)) mutableState.value = mutableState.value.copy(loading = false)
                }
            }
    }

    private fun canRead() = !closed && hub != null && binding != null && runtime != null

    private fun isCurrent(token: Long) = !closed && generation == token
}

private fun ActualDraft.unconfirmed() = copy(fields = fields.copy(confirmed = false))

private fun ActualRequest.lookupKind() = if (this is ActualRequest.Report) ActualLookupKind.Report else ActualLookupKind.Review

private val ACTUAL_REJECTIONS =
    setOf(
        ApplicationErrorCode.HubInvalidInput,
        ApplicationErrorCode.HubConflict,
        ApplicationErrorCode.HubCapacity,
        ApplicationErrorCode.HubPermissionDenied,
        ApplicationErrorCode.CredentialMissing,
        ApplicationErrorCode.KeyringUnavailable,
    )
