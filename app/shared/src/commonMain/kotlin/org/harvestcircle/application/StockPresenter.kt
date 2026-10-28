package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlin.time.Instant

data class ReceiptDraft(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val variantId: String? = null,
    val supplierId: String? = null,
    val producerId: String? = null,
    val receivingId: String? = null,
    val plannedLocationId: String? = null,
    val quantity: String = "",
    val observation: String = "",
    val existingSourceId: String? = null,
    val sourceCertainty: HubSourceCertainty = HubSourceCertainty.Unknown,
    val sourceKind: HubSourceKind = HubSourceKind.ProducerBatch,
    val issuerId: String? = null,
    val sourceCode: String = "",
    val provenance: String = "",
    val occurrenceKind: HubOccurrenceKind = HubOccurrenceKind.Unknown,
    val occurrenceStart: String = "",
    val occurrenceEnd: String = "",
)

data class ReceiptAttempt(
    val operation: OperationId,
    val input: HubReceiptInput,
)

sealed interface ReceiptSubmission {
    val attempt: ReceiptAttempt

    data class Pending(
        override val attempt: ReceiptAttempt,
    ) : ReceiptSubmission

    data class Uncertain(
        override val attempt: ReceiptAttempt,
        val message: String,
    ) : ReceiptSubmission

    data class Rejected(
        override val attempt: ReceiptAttempt,
        val problem: ApplicationProblem,
    ) : ReceiptSubmission

    data class Recorded(
        override val attempt: ReceiptAttempt,
        val receipt: HubStockReceipt,
    ) : ReceiptSubmission
}

data class StockContextReview(
    val page: HubStockPage,
    val references: List<HubReferenceRecord>,
    val query: HubStockQuery,
)

data class StockViewState(
    val available: Boolean = false,
    val loading: Boolean = false,
    val page: HubStockPage? = null,
    val references: List<HubReferenceRecord> = emptyList(),
    val referenceRevision: ULong? = null,
    val queryText: String = "",
    val locationFilter: String? = null,
    val inspecting: HubStockRow? = null,
    val draft: ReceiptDraft? = null,
    val receiptOpen: Boolean = false,
    val submission: ReceiptSubmission? = null,
    val needsReview: Boolean = false,
    val review: StockContextReview? = null,
    val message: String? = null,
)

/** UI-scoped coordination. All stock, work, provenance and permission decisions stay native. */
class StockPresenter(
    private val runtime: HarvestCircleStockRuntime?,
    private val references: HarvestCircleReferenceRuntime?,
    private val scope: CoroutineScope,
    private val operationIds: OperationIdSource,
) {
    private val mutableState = MutableStateFlow(StockViewState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var closed = false
    private var generation = 0L
    private var job: Job? = null
    private var loadedQuery: HubStockQuery? = null

    fun attach(hubId: String) {
        if (closed || runtime == null || references == null || hubId == hub) return
        generation += 1
        job?.cancel()
        hub = hubId
        mutableState.value = StockViewState(available = true)
        refresh()
    }

    fun search(text: String) {
        if (text.length <= 1024) mutableState.value = mutableState.value.copy(queryText = text)
    }

    fun filterLocation(id: String?) {
        if (mutableState.value.submission != null || mutableState.value.draft != null) return
        mutableState.value = mutableState.value.copy(locationFilter = id)
        refresh()
    }

    fun refresh() = loadPage(viewQuery())

    fun nextPage() {
        val page = mutableState.value.page ?: return
        if (!page.more) return
        val query = loadedQuery ?: return
        loadPage(
            query.copy(
                expectedStockRevision = page.stockRevision,
                expectedCatalogRevision = page.catalogRevision,
                offset = page.offset + page.rows.size.toUInt(),
            ),
        )
    }

    private fun viewQuery() = HubStockQuery(locationId = mutableState.value.locationFilter, text = mutableState.value.queryText)

    private fun loadPage(query: HubStockQuery) {
        if (!canRead() || mutableState.value.submission is ReceiptSubmission.Pending) return
        mutableState.value = mutableState.value.copy(page = null, inspecting = null)
        loadedQuery = null
        read {
            val page = requireNotNull(runtime).stock(query)
            val records = referenceChoices(page.catalogRevision)
            val update: (StockViewState) -> StockViewState = { current ->
                loadedQuery = query
                current.copy(
                    page = page,
                    references = records,
                    referenceRevision = page.catalogRevision,
                    needsReview =
                        current.needsReview ||
                            current.draft?.let {
                                it.stockRevision != page.stockRevision || it.catalogRevision != page.catalogRevision
                            } == true,
                )
            }
            update
        }
    }

    private suspend fun referenceChoices(revision: ULong): List<HubReferenceRecord> {
        val current = mutableState.value
        if (current.referenceRevision == revision) return current.references
        val records = mutableListOf<HubReferenceRecord>()
        var offset = 0U
        repeat(8) {
            val page = requireNotNull(references).references(HubReferenceQuery(expectedCatalogRevision = revision, offset = offset))
            check(page.catalogRevision == revision && page.offset == offset && page.total <= 512U)
            records += page.records
            check(records.size <= 512)
            if (!page.more) {
                check(records.size.toUInt() == page.total)
                return records.toList()
            }
            check(page.records.isNotEmpty())
            offset += page.records.size.toUInt()
        }
        error("Reference choices exceed the bounded catalog")
    }

    fun beginReceipt(source: HubStockRow? = null) {
        val state = mutableState.value
        val page = state.page ?: return
        if (!canRead() || state.loading || state.submission != null) return
        if (state.draft != null) {
            mutableState.value = state.copy(receiptOpen = true, inspecting = null)
            return
        }
        val original = source?.trace?.source
        mutableState.value =
            state.copy(
                draft =
                    ReceiptDraft(
                        page.stockRevision,
                        page.catalogRevision,
                        variantId = source?.trace?.variant?.id,
                        existingSourceId = original?.id,
                        issuerId = original?.issuer?.id,
                        sourceCertainty = original?.certainty ?: HubSourceCertainty.Unknown,
                        sourceKind = original?.kind ?: HubSourceKind.ProducerBatch,
                        sourceCode = original?.code.orEmpty(),
                        provenance = original?.provenance.orEmpty(),
                    ),
                inspecting = null,
                receiptOpen = true,
                needsReview = false,
                review = null,
                message = null,
            )
    }

    fun updateDraft(transform: (ReceiptDraft) -> ReceiptDraft) {
        val state = mutableState.value
        val draft = state.draft ?: return
        if (state.submission != null) return
        val changed = transform(draft)
        if (listOf(
                changed.quantity,
                changed.observation,
                changed.sourceCode,
                changed.provenance,
                changed.occurrenceStart,
                changed.occurrenceEnd,
            ).any { it.length > 1024 }
        ) {
            return
        }
        mutableState.value = state.copy(draft = changed, message = null)
    }

    fun closeDraft() {
        if (mutableState.value.submission != null) return
        generation += 1
        job?.cancel()
        mutableState.value =
            mutableState.value.copy(draft = null, receiptOpen = false, needsReview = false, review = null, loading = false, message = null)
    }

    fun submitReceipt(snapshot: SnapshotRevision) {
        val current = mutableState.value
        val draft = current.draft ?: return
        if (!canRead() || current.loading || current.submission != null) return
        if (current.needsReview) {
            mutableState.value = current.copy(message = "Review current stock and reference versions before submitting retained input.")
            return
        }
        val input =
            try {
                draft.input()
            } catch (error: IllegalArgumentException) {
                mutableState.value =
                    current.copy(message = "Choose an original pack and Receiving area, enter a whole count, and check UTC time fields.")
                return
            }
        val attempt = ReceiptAttempt(operationIds.next(), input)
        mutableState.value = current.copy(submission = ReceiptSubmission.Pending(attempt), message = null)
        val token = generation
        job =
            scope.launch {
                try {
                    val receipt = requireNotNull(runtime).receiveStock(RequestContext(attempt.operation, snapshot, 10_000UL), attempt.input)
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(submission = ReceiptSubmission.Recorded(attempt, receipt))
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        val problem = (error as? ApplicationFailure)?.problem
                        val result =
                            if (problem != null && problem.code in RECEIPT_REJECTIONS) {
                                ReceiptSubmission.Rejected(attempt, problem)
                            } else {
                                ReceiptSubmission.Uncertain(
                                    attempt,
                                    problem?.safeMessage ?: "The receipt outcome is unconfirmed. Check its original result.",
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
        if (submitted is ReceiptSubmission.Pending || submitted is ReceiptSubmission.Recorded) return
        read {
            val receipt = requireNotNull(runtime).receiptOperation(submitted.attempt.operation)
            val result =
                if (receipt == null && submitted is ReceiptSubmission.Rejected) {
                    submitted
                } else if (receipt == null) {
                    ReceiptSubmission.Uncertain(
                        submitted.attempt,
                        "No receipt is visible. Older restored history cannot prove absence. Keep the original operation for review; do not submit another ID.",
                    )
                } else {
                    ReceiptSubmission.Recorded(submitted.attempt, receipt)
                }
            val update: (StockViewState) -> StockViewState = { it.copy(submission = result) }
            update
        }
    }

    fun reviewRejected() {
        if (mutableState.value.submission !is ReceiptSubmission.Rejected) return
        mutableState.value =
            mutableState.value.copy(
                submission = null,
                message = "Your input is retained. Review the reported problem before another submission.",
            )
    }

    fun reviewContext() {
        if (mutableState.value.draft == null || mutableState.value.submission != null) return
        val query = viewQuery()
        read {
            val page = requireNotNull(runtime).stock(query)
            val records = referenceChoices(page.catalogRevision)
            val update: (StockViewState) -> StockViewState = { it.copy(review = StockContextReview(page, records, query)) }
            update
        }
    }

    fun acceptReview() {
        val state = mutableState.value
        val draft = state.draft ?: return
        val review = state.review ?: return
        if (state.loading || state.submission != null) return
        loadedQuery = review.query
        mutableState.value =
            state.copy(
                page = review.page,
                references = review.references,
                referenceRevision = review.page.catalogRevision,
                draft = draft.copy(stockRevision = review.page.stockRevision, catalogRevision = review.page.catalogRevision),
                review = null,
                needsReview = false,
                message = "Current context reviewed. Your receipt input is retained.",
            )
    }

    fun finishRecorded() {
        if (mutableState.value.submission !is ReceiptSubmission.Recorded) return
        mutableState.value =
            mutableState.value.copy(
                submission = null,
                draft = null,
                receiptOpen = false,
                review = null,
                needsReview = false,
                message = null,
            )
        refresh()
    }

    fun inspect(row: HubStockRow) {
        if (mutableState.value.submission != null || mutableState.value.loading) return
        mutableState.value = mutableState.value.copy(inspecting = row)
    }

    fun hideReceipt() {
        mutableState.value = mutableState.value.copy(receiptOpen = false)
    }

    fun closeInspection() {
        mutableState.value = mutableState.value.copy(inspecting = null)
    }

    private fun read(operation: suspend () -> ((StockViewState) -> StockViewState)) {
        if (!canRead() || mutableState.value.submission is ReceiptSubmission.Pending) return
        generation += 1
        val token = generation
        job?.cancel()
        mutableState.value = mutableState.value.copy(loading = true, message = null)
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
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "Local stock and reference choices could not be read. Refresh when the application is ready.",
                            )
                    }
                } finally {
                    if (isCurrent(token)) mutableState.value = mutableState.value.copy(loading = false)
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
        mutableState.value = StockViewState()
    }
}

private fun ReceiptDraft.input(): HubReceiptInput {
    val occurrence =
        when (occurrenceKind) {
            HubOccurrenceKind.Unknown -> HubOccurrence(HubOccurrenceKind.Unknown)
            HubOccurrenceKind.Known -> HubOccurrence(HubOccurrenceKind.Known, Instant.parse(occurrenceStart.trim()).epochSeconds)
            HubOccurrenceKind.Interval ->
                HubOccurrence(
                    HubOccurrenceKind.Interval,
                    Instant.parse(occurrenceStart.trim()).epochSeconds,
                    Instant.parse(occurrenceEnd.trim()).epochSeconds,
                )
        }
    val source =
        if (sourceCertainty == HubSourceCertainty.Unknown) {
            HubSourceInput()
        } else {
            HubSourceInput(existingSourceId, issuerId, sourceKind, sourceCode, provenance, sourceCertainty)
        }
    return HubReceiptInput(
        stockRevision,
        catalogRevision,
        requireNotNull(variantId),
        supplierId,
        producerId,
        source,
        requireNotNull(quantity.toUIntOrNull()),
        requireNotNull(receivingId),
        plannedLocationId,
        occurrence,
        observation,
    )
}

private val RECEIPT_REJECTIONS =
    setOf(
        ApplicationErrorCode.HubInvalidInput,
        ApplicationErrorCode.HubConflict,
        ApplicationErrorCode.HubCapacity,
        ApplicationErrorCode.HubPermissionDenied,
        ApplicationErrorCode.CredentialMissing,
        ApplicationErrorCode.KeyringUnavailable,
    )
