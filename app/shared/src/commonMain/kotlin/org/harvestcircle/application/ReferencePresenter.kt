package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

data class ReferenceDraft(
    val catalogRevision: ULong,
    val kind: HubReferenceKind,
    val recordId: String? = null,
    val recordRevision: ULong? = null,
    val label: String = "",
    val productId: String? = null,
    val nominalGrams: String = "",
    val locationPurpose: HubLocationPurpose = HubLocationPurpose.Storage,
)

data class ReferenceContextReview(
    val catalogRevision: ULong,
    val current: HubReferenceRecord?,
)

data class ReferenceAttempt(
    val operation: OperationId,
    val edit: HubReferenceEdit,
)

sealed interface ReferenceSubmission {
    val attempt: ReferenceAttempt

    data class Pending(
        override val attempt: ReferenceAttempt,
    ) : ReferenceSubmission

    data class Uncertain(
        override val attempt: ReferenceAttempt,
        val message: String,
    ) : ReferenceSubmission

    data class Rejected(
        override val attempt: ReferenceAttempt,
        val problem: ApplicationProblem,
    ) : ReferenceSubmission

    data class Recorded(
        override val attempt: ReferenceAttempt,
        val receipt: HubReferenceReceipt,
    ) : ReferenceSubmission
}

data class ReferenceViewState(
    val available: Boolean = false,
    val loading: Boolean = false,
    val kind: HubReferenceKind = HubReferenceKind.Product,
    val queryText: String = "",
    val page: HubReferencePage? = null,
    val draft: ReferenceDraft? = null,
    val submission: ReferenceSubmission? = null,
    val contextReview: ReferenceContextReview? = null,
    val draftNeedsReview: Boolean = false,
    val message: String? = null,
    val products: HubReferencePage? = null,
    val productSearch: String = "",
    val inspecting: HubReferenceRecord? = null,
    val history: List<HubReferenceReceipt> = emptyList(),
    val historyMore: Boolean = false,
)

/** Confined to the owning UI scope. It coordinates drafts, never stock or authority. */
class ReferencePresenter(
    private val runtime: HarvestCircleReferenceRuntime?,
    private val scope: CoroutineScope,
    private val operationIds: OperationIdSource,
) {
    private val mutableState = MutableStateFlow(ReferenceViewState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var closed = false
    private var generation = 0L
    private var job: Job? = null
    private var productJob: Job? = null
    private var productGeneration = 0L
    private var loadedQuery: HubReferenceQuery? = null
    private var loadedProductQuery: HubReferenceQuery? = null

    fun attach(hubId: String) {
        if (closed || runtime == null || hub == hubId) return
        generation += 1
        job?.cancel()
        productJob?.cancel()
        productGeneration += 1
        hub = hubId
        mutableState.value = ReferenceViewState(available = true)
        reload()
    }

    fun selectKind(kind: HubReferenceKind) {
        if (!canRead() || mutableState.value.submission is ReferenceSubmission.Pending) return
        mutableState.value = mutableState.value.copy(kind = kind, inspecting = null, history = emptyList())
        reload()
    }

    fun search(text: String) {
        if (text.length > 1024) return
        mutableState.value = mutableState.value.copy(queryText = text)
    }

    fun reload() = loadPage(HubReferenceQuery(kind = mutableState.value.kind, text = mutableState.value.queryText))

    fun nextPage() {
        val state = mutableState.value
        val page = state.page ?: return
        if (!page.more) return
        loadPage(
            requireNotNull(loadedQuery).copy(
                expectedCatalogRevision = page.catalogRevision,
                offset =
                    page.offset + page.records.size.toUInt(),
            ),
        )
    }

    fun firstPage() = reload()

    private fun loadPage(query: HubReferenceQuery) {
        if (!canRead() || mutableState.value.submission is ReferenceSubmission.Pending) return
        mutableState.value = mutableState.value.copy(page = null)
        loadedQuery = null
        launchRead {
            val page = requireNotNull(runtime).references(query)
            val update: (ReferenceViewState) -> ReferenceViewState = { state ->
                loadedQuery = query
                state.copy(page = page)
            }
            update
        }
    }

    fun create() {
        val state = mutableState.value
        val page = state.page ?: return
        if (state.loading || state.submission != null) return
        mutableState.value =
            state.copy(
                draft = ReferenceDraft(page.catalogRevision, state.kind),
                draftNeedsReview = false,
                contextReview = null,
                inspecting = null,
                message = null,
            )
        if (state.kind == HubReferenceKind.Pack) searchProducts("")
    }

    fun edit(record: HubReferenceRecord) {
        val state = mutableState.value
        val page = state.page ?: return
        if (state.loading || state.submission != null || record.value.kind == HubReferenceKind.Pack) return
        val value = record.value
        mutableState.value =
            state.copy(
                draft =
                    ReferenceDraft(
                        page.catalogRevision,
                        value.kind,
                        record.id,
                        record.revision,
                        value.label,
                        value.productId,
                        value.nominalGrams?.toString().orEmpty(),
                        value.locationPurpose ?: HubLocationPurpose.Storage,
                    ),
                inspecting = null,
                draftNeedsReview = false,
                contextReview = null,
                message = null,
            )
    }

    fun updateDraft(transform: (ReferenceDraft) -> ReferenceDraft) {
        val state = mutableState.value
        if (state.submission != null) return
        val draft = state.draft ?: return
        val changed = transform(draft)
        if (changed.label.length > 1024 || changed.nominalGrams.length > 64) return
        mutableState.value = state.copy(draft = changed, message = null)
    }

    fun reviewDraftContext() {
        if (mutableState.value.draft == null || mutableState.value.submission != null) return
        launchRead {
            val draft = requireNotNull(mutableState.value.draft)
            var page = requireNotNull(runtime).references(HubReferenceQuery(kind = draft.kind))
            var current = page.records.find { it.id == draft.recordId }
            var pages = 1
            while (draft.recordId != null && current == null && page.more && pages < 8) {
                page =
                    runtime.references(HubReferenceQuery(page.catalogRevision, page.offset + page.records.size.toUInt(), kind = draft.kind))
                current = page.records.find { it.id == draft.recordId }
                pages += 1
            }
            check(draft.recordId == null || current != null) { "Current reference is unavailable" }
            val review = ReferenceContextReview(page.catalogRevision, current)
            val update: (ReferenceViewState) -> ReferenceViewState = { it.copy(contextReview = review) }
            update
        }
    }

    fun acceptContextReview() {
        val state = mutableState.value
        if (state.submission != null || state.loading) return
        val draft = state.draft ?: return
        val review = state.contextReview ?: return
        mutableState.value =
            state.copy(
                draft = draft.copy(catalogRevision = review.catalogRevision, recordRevision = review.current?.revision),
                contextReview = null,
                draftNeedsReview = false,
                message = "Current versions reviewed. Your input is retained.",
            )
    }

    fun closeDraft() {
        if (mutableState.value.submission != null) return
        generation += 1
        job?.cancel()
        productJob?.cancel()
        mutableState.value =
            mutableState.value.copy(draft = null, message = null, loading = false, contextReview = null, draftNeedsReview = false)
    }

    fun submit(snapshotRevision: SnapshotRevision) {
        val state = mutableState.value
        val draft = state.draft ?: return
        if (!canRead() || state.loading || state.submission != null) return
        if (state.draftNeedsReview) {
            mutableState.value = state.copy(message = "Review current reference versions before submitting this retained input.")
            return
        }
        val grams = draft.nominalGrams.takeIf(String::isNotEmpty)?.toUIntOrNull()
        if (draft.kind == HubReferenceKind.Pack && draft.nominalGrams.isNotEmpty() && grams == null) {
            mutableState.value = state.copy(message = "Enter whole nominal grams, or leave the mass unknown.")
            return
        }
        val value =
            HubReferenceValue(
                draft.kind,
                draft.label,
                productId = draft.productId.takeIf { draft.kind == HubReferenceKind.Pack },
                nominalGrams = grams.takeIf { draft.kind == HubReferenceKind.Pack },
                locationPurpose = draft.locationPurpose.takeIf { draft.kind == HubReferenceKind.Location },
            )
        val attempt =
            ReferenceAttempt(operationIds.next(), HubReferenceEdit(draft.catalogRevision, draft.recordId, draft.recordRevision, value))
        mutableState.value = state.copy(submission = ReferenceSubmission.Pending(attempt), message = null)
        val token = generation
        job =
            scope.launch {
                try {
                    val receipt =
                        requireNotNull(
                            runtime,
                        ).editReference(RequestContext(attempt.operation, snapshotRevision, 10_000UL), attempt.edit)
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(submission = ReferenceSubmission.Recorded(attempt, receipt))
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        val problem = (error as? ApplicationFailure)?.problem
                        val submission =
                            if (problem != null && problem.code in KNOWN_REJECTIONS) {
                                ReferenceSubmission.Rejected(attempt, problem)
                            } else {
                                ReferenceSubmission.Uncertain(
                                    attempt,
                                    problem?.safeMessage ?: "The operation outcome is unconfirmed. Check its original result.",
                                )
                            }
                        mutableState.value =
                            mutableState.value.copy(
                                submission = submission,
                                draftNeedsReview = problem?.code == ApplicationErrorCode.HubConflict,
                            )
                    }
                }
            }
    }

    fun checkOriginal() {
        val submission = mutableState.value.submission ?: return
        if (submission is ReferenceSubmission.Pending || submission is ReferenceSubmission.Recorded) return
        launchRead {
            val receipt = requireNotNull(runtime).referenceOperation(submission.attempt.operation)
            val result =
                if (receipt == null) {
                    ReferenceSubmission.Uncertain(
                        submission.attempt,
                        "No receipt is visible. Retain this operation for review; restored history cannot prove absence. Do not use another ID.",
                    )
                } else {
                    ReferenceSubmission.Recorded(submission.attempt, receipt)
                }
            val update: (ReferenceViewState) -> ReferenceViewState = { it.copy(submission = result) }
            update
        }
    }

    fun reviewRejected() {
        if (mutableState.value.submission !is ReferenceSubmission.Rejected) return
        mutableState.value =
            mutableState.value.copy(
                submission = null,
                message = "Input retained. Review current reference versions before a new submission.",
            )
    }

    fun finishRecorded() {
        if (mutableState.value.submission !is ReferenceSubmission.Recorded) return
        mutableState.value = mutableState.value.copy(submission = null, draft = null, message = null)
        reload()
    }

    fun inspect(record: HubReferenceRecord) {
        if (!canRead() || mutableState.value.submission != null) return
        mutableState.value = mutableState.value.copy(inspecting = record, history = emptyList(), historyMore = false)
        loadHistory(0UL)
    }

    fun closeInspection() {
        if (mutableState.value.submission is ReferenceSubmission.Pending) return
        generation += 1
        job?.cancel()
        mutableState.value = mutableState.value.copy(inspecting = null, history = emptyList(), historyMore = false, loading = false)
    }

    fun moreHistory() {
        val last =
            mutableState.value.history
                .lastOrNull()
                ?.current
                ?.revision ?: return
        loadHistory(last)
    }

    private fun loadHistory(after: ULong) =
        launchRead {
            val record = requireNotNull(mutableState.value.inspecting)
            val history = requireNotNull(runtime).referenceHistory(record.id, after, 64U)
            val update: (ReferenceViewState) -> ReferenceViewState = { state ->
                state.copy(
                    history = if (after == 0UL) history else state.history + history,
                    historyMore = history.size == 64,
                )
            }
            update
        }

    fun searchProducts(
        text: String,
        next: Boolean = false,
    ) {
        if (!canRead() || text.length > 1024) return
        val previous = mutableState.value.products
        if (next && (previous?.more != true || loadedProductQuery?.text != text)) return
        productGeneration += 1
        val productToken = productGeneration
        val token = generation
        productJob?.cancel()
        mutableState.value = mutableState.value.copy(productSearch = text)
        productJob =
            scope.launch {
                try {
                    val query =
                        if (next && previous != null) {
                            HubReferenceQuery(
                                previous.catalogRevision,
                                previous.offset + previous.records.size.toUInt(),
                                kind = HubReferenceKind.Product,
                                text = text,
                            )
                        } else {
                            HubReferenceQuery(kind = HubReferenceKind.Product, text = text)
                        }
                    val page = requireNotNull(runtime).references(query)
                    if (isCurrent(token) && productToken == productGeneration) {
                        loadedProductQuery = query
                        mutableState.value = mutableState.value.copy(products = page)
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token) &&
                        productToken == productGeneration
                    ) {
                        mutableState.value = mutableState.value.copy(message = safeMessage(error))
                    }
                }
            }
    }

    fun changeProductSearch(text: String) {
        if (text.length <= 1024) mutableState.value = mutableState.value.copy(productSearch = text)
    }

    private fun launchRead(operation: suspend () -> ((ReferenceViewState) -> ReferenceViewState)) {
        if (!canRead() || mutableState.value.submission is ReferenceSubmission.Pending) return
        generation += 1
        val token = generation
        job?.cancel()
        mutableState.value = mutableState.value.copy(loading = true, message = null)
        job =
            scope.launch {
                try {
                    val update = operation()
                    if (isCurrent(token)) mutableState.value = update(mutableState.value)
                } catch (
                    error: CancellationException,
                ) {
                    throw error
                } catch (
                    error: Exception,
                ) {
                    if (isCurrent(token)) mutableState.value = mutableState.value.copy(message = safeMessage(error))
                } finally {
                    if (isCurrent(token)) mutableState.value = mutableState.value.copy(loading = false)
                }
            }
    }

    private fun canRead(): Boolean = !closed && hub != null && runtime != null

    private fun isCurrent(token: Long): Boolean = !closed && token == generation

    private fun safeMessage(error: Exception): String =
        (error as? ApplicationFailure)?.problem?.safeMessage ?: "The reference data could not be read. Check the application status."

    fun close() {
        closed = true
        generation += 1
        productGeneration += 1
        job?.cancel()
        productJob?.cancel()
        hub = null
        mutableState.value = ReferenceViewState()
    }
}

private val KNOWN_REJECTIONS =
    setOf(
        ApplicationErrorCode.HubInvalidInput,
        ApplicationErrorCode.HubConflict,
        ApplicationErrorCode.HubCapacity,
        ApplicationErrorCode.HubPermissionDenied,
        ApplicationErrorCode.CredentialMissing,
        ApplicationErrorCode.KeyringUnavailable,
    )
