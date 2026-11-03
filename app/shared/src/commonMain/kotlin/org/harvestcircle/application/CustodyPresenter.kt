package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** Captures caller-owned attempts; archives and missing originals never become replay grants. */
class CustodyPresenter(
    private val runtime: HarvestCircleCustodyRuntime?,
    private val stock: HarvestCircleStockRuntime?,
    private val handling: HarvestCircleHandlingRuntime?,
    private val references: HarvestCircleReferenceRuntime?,
    private val departures: HarvestCircleDepartureRuntime?,
    private val scope: CoroutineScope,
    private val operationIds: OperationIdSource,
    private val onFinished: () -> Unit = {},
) {
    private val mutableState = MutableStateFlow(CustodyViewState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var binding: NativeSessionBinding? = null
    private var generation = 0L
    private var closed = false
    private var job: Job? = null

    /** A replaced native binding requires a new owner, even when identity text is equal. */
    fun attach(
        id: String,
        native: NativeSessionBinding,
    ) {
        if (closed || runtime == null || stock == null || handling == null || references == null) return
        if (hub != null) {
            if (hub != id || binding != native) close()
            return
        }
        hub = id
        binding = native
        mutableState.value = CustodyViewState(available = true)
    }

    fun beginReturn() = begin(CustodyTarget.Return)

    fun beginDisposition(group: String) = begin(CustodyTarget.Dispose(group))

    private fun begin(target: CustodyTarget) {
        val current = mutableState.value
        if (!canRead() || current.loading) return
        if (current.target != null || current.originalLookup != null || current.submission != null) {
            resume()
            return
        }
        mutableState.value = CustodyViewState(available = true, open = true, target = target)
        refresh()
    }

    fun edit(transform: (CustodyFields) -> CustodyFields) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.loading || current.submission != null) return
        val changed = transform(draft.fields)
        val fields = changed.copy(common = changed.common.copy(confirmed = false))
        if (!fields.bounded()) return
        mutableState.value =
            current.copy(
                draft =
                    draft.copy(
                        fields = fields,
                        departure =
                            draft.departure.takeIf {
                                fields.departureId == draft.fields.departureId &&
                                    fields.departureOperation == draft.fields.departureOperation
                            },
                    ),
                message = null,
            )
    }

    fun confirm(value: Boolean) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (current.loading || current.needsReview || current.submission != null) return
        mutableState.value =
            current.copy(draft = draft.copy(fields = draft.fields.copy(common = draft.fields.common.copy(confirmed = value))))
    }

    fun refresh() {
        val current = mutableState.value
        val target = current.target ?: return
        if (current.submission != null) return
        mutableState.value = current.copy(needsReview = current.draft != null, review = null, draft = current.draft?.unconfirmed())
        read {
            val view = (target as? CustodyTarget.Dispose)?.let { requireNotNull(handling).group(it.groupId) }
            val page = if (view == null) requireNotNull(stock).stock(HubStockQuery(limit = 1U)) else null
            val stockRevision = view?.stockRevision ?: requireNotNull(page).stockRevision
            val catalogRevision = view?.catalogRevision ?: requireNotNull(page).catalogRevision
            val context = CustodyContext(stockRevision, catalogRevision, referenceChoices(catalogRevision), view)
            val update: (CustodyViewState) -> CustodyViewState = { state ->
                if (state.draft == null) {
                    state.copy(draft = CustodyDraft(context), needsReview = false)
                } else {
                    state.copy(review = context)
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
                draft = draft.unconfirmed().copy(context = review),
                review = null,
                needsReview = false,
                message = "Current native context reviewed. Confirm the actual facts again.",
            )
    }

    fun readDeparture() {
        val draft = mutableState.value.draft ?: return
        if (mutableState.value.target != CustodyTarget.Return || mutableState.value.submission != null) return
        val operation = parseOperation(draft.fields.departureOperation) ?: return
        read {
            val original = requireNotNull(departures).departureOperation(operation)
            if (original != null) check(original.operationId == operation)
            val update: (CustodyViewState) -> CustodyViewState = { state ->
                val current = requireNotNull(state.draft)
                state.copy(
                    draft =
                        if (original == null) {
                            current.unconfirmed().copy(departure = null)
                        } else {
                            current.unconfirmed().copy(
                                fields =
                                    current.fields.copy(
                                        departureId = original.id,
                                        originalGroupId = "",
                                        common = current.fields.common.copy(confirmed = false),
                                    ),
                                departure = original,
                            )
                        },
                    message =
                        if (original == null) {
                            "No original departure is visible. Its absence does not establish what happened."
                        } else {
                            "Original departure read. Choose its group explicitly or leave it unknown. Source and treatment are separate."
                        },
                )
            }
            update
        }
    }

    fun submit(snapshot: SnapshotRevision) {
        val current = mutableState.value
        val draft = current.draft ?: return
        val target = current.target ?: return
        if (!canRead() || current.loading || current.needsReview || current.submission != null || !draft.fields.common.confirmed) return
        val request =
            try {
                draft.request(target)
            } catch (_: IllegalArgumentException) {
                mutableState.value =
                    current.copy(
                        message = "Check pack, location, whole quantity, physical identification, association and UTC occurrence.",
                    )
                return
            }
        val attempt = CustodyAttempt(operationIds.next(), request)
        val token = generation
        mutableState.value = current.copy(submission = CustodySubmission.Pending(attempt), message = null)
        job =
            scope.launch {
                try {
                    val context = RequestContext(attempt.operation, snapshot, 10_000UL)
                    val service = requireNotNull(runtime)
                    val result =
                        when (request) {
                            is CustodyRequest.Return -> CustodyResult.Returned(service.applyReturn(context, request.input))
                            is CustodyRequest.Dispose -> CustodyResult.Disposed(service.applyDisposition(context, request.input))
                        }
                    check(result.operation == attempt.operation)
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(submission = CustodySubmission.Recorded(attempt, result))
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        val problem = (error as? ApplicationFailure)?.problem
                        val outcome =
                            if (problem != null && problem.code in CUSTODY_REJECTIONS) {
                                CustodySubmission.Rejected(attempt, problem)
                            } else {
                                CustodySubmission.Uncertain(
                                    attempt,
                                    problem?.safeMessage ?: "The original custody operation is unconfirmed. Read its original result.",
                                )
                            }
                        mutableState.value = mutableState.value.copy(submission = outcome)
                    }
                }
            }
    }

    fun checkOriginal() {
        val submitted = mutableState.value.submission ?: return
        if (submitted is CustodySubmission.Pending || submitted is CustodySubmission.Recorded) return
        read {
            val result =
                original(
                    submitted.attempt.operation,
                    if (submitted.attempt.request is CustodyRequest.Return) CustodyLookupKind.Return else CustodyLookupKind.Disposition,
                )
            val outcome =
                when {
                    result != null -> CustodySubmission.Recorded(submitted.attempt, result)
                    submitted is CustodySubmission.Rejected -> submitted
                    else ->
                        CustodySubmission.Uncertain(
                            submitted.attempt,
                            "No original is visible. Retain this operation; absence does not authorize another submission.",
                        )
                }
            val update: (CustodyViewState) -> CustodyViewState = { it.copy(submission = outcome) }
            update
        }
    }

    fun reviewRejected() {
        val current = mutableState.value
        if (current.submission !is CustodySubmission.Rejected) return
        mutableState.value = current.copy(submission = null, needsReview = true, draft = current.draft?.unconfirmed())
        refresh()
    }

    fun beginOriginalLookup() {
        val current = mutableState.value
        if (!canRead() || current.loading) return
        if (current.target != null || current.submission != null || current.originalLookup != null) {
            resume()
            return
        }
        mutableState.value = CustodyViewState(available = true, open = true, originalLookup = CustodyOriginalLookup())
    }

    fun editOriginalLookup(
        kind: CustodyLookupKind,
        text: String,
    ) {
        val current = mutableState.value
        if (current.originalLookup == null || current.loading || text.length > 36) return
        mutableState.value = current.copy(originalLookup = CustodyOriginalLookup(kind, text), message = null)
    }

    fun lookupOriginal() {
        val lookup = mutableState.value.originalLookup ?: return
        val operation = parseOperation(lookup.operationText) ?: return
        read {
            val result = original(operation, lookup.kind)
            val update: (CustodyViewState) -> CustodyViewState = {
                it.copy(
                    originalLookup = lookup.copy(result = result),
                    message =
                        if (result ==
                            null
                        ) {
                            "No original of this kind is visible. Absence grants no resubmission permission."
                        } else {
                            "Original read without another submission. Historical facts grant no current mutation permission."
                        },
                )
            }
            update
        }
    }

    private suspend fun original(
        operation: OperationId,
        kind: CustodyLookupKind,
    ): CustodyResult? {
        val service = requireNotNull(runtime)
        val result =
            when (kind) {
                CustodyLookupKind.Return -> service.returnOperation(operation)?.let(CustodyResult::Returned)
                CustodyLookupKind.Disposition -> service.dispositionOperation(operation)?.let(CustodyResult::Disposed)
            }
        if (result != null) check(result.operation == operation)
        return result
    }

    private fun parseOperation(text: String): OperationId? =
        try {
            OperationId.from(text)
        } catch (_: IllegalArgumentException) {
            mutableState.value = mutableState.value.copy(message = "Enter the complete canonical original operation ID.")
            null
        }

    fun finishRecorded() {
        if (mutableState.value.submission !is CustodySubmission.Recorded) return
        mutableState.value = CustodyViewState(available = true)
        onFinished()
    }

    fun park() {
        val current = mutableState.value
        if (current.submission != null) return
        generation += 1
        job?.cancel()
        mutableState.value =
            current.copy(
                open = false,
                loading = false,
                needsReview = current.draft != null,
                review = null,
                draft = current.draft?.unconfirmed(),
            )
    }

    fun resume() {
        if (!canRead() || mutableState.value.loading) return
        mutableState.value = mutableState.value.copy(open = true)
        if (mutableState.value.submission == null && mutableState.value.target != null) refresh()
    }

    fun discard() {
        val current = mutableState.value
        if (current.loading || current.submission != null) return
        generation += 1
        job?.cancel()
        mutableState.value = CustodyViewState(available = true)
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

    private fun read(operation: suspend () -> ((CustodyViewState) -> CustodyViewState)) {
        val current = mutableState.value
        if (!canRead() || current.loading || current.submission is CustodySubmission.Pending) return
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
                                review = null,
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "Current custody context is unavailable. Input and original operation are retained.",
                            )
                    }
                } finally {
                    if (isCurrent(token)) mutableState.value = mutableState.value.copy(loading = false)
                }
            }
    }

    private fun canRead() = !closed && hub != null && binding != null && runtime != null

    private fun isCurrent(token: Long) = !closed && token == generation

    fun close() {
        closed = true
        generation += 1
        job?.cancel()
        hub = null
        binding = null
        mutableState.value = CustodyViewState()
    }
}

private fun CustodyDraft.unconfirmed() = copy(fields = fields.copy(common = fields.common.copy(confirmed = false)))

private val CUSTODY_REJECTIONS =
    setOf(
        ApplicationErrorCode.HubInvalidInput,
        ApplicationErrorCode.HubConflict,
        ApplicationErrorCode.HubCapacity,
        ApplicationErrorCode.HubPermissionDenied,
        ApplicationErrorCode.CredentialMissing,
        ApplicationErrorCode.KeyringUnavailable,
    )
