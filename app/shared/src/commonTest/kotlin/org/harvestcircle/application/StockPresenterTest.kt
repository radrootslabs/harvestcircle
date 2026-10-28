package org.harvestcircle.application

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class StockPresenterTest {
    @Test
    fun lostReceiptReplyFreezesOriginalInputAndOnlyReadsItsOutcome() =
        runTest {
            val runtime = StockFake()
            var ids = 0
            val presenter =
                StockPresenter(
                    runtime,
                    runtime,
                    this,
                    OperationIdSource {
                        ids += 1
                        StockFixtures.operation
                    },
                )
            presenter.attach("hub")
            runCurrent()
            presenter.beginReceipt()
            presenter.updateDraft(StockFixtures::readyDraft)
            presenter.submitReceipt(SnapshotRevision(7UL))
            presenter.submitReceipt(SnapshotRevision(7UL))
            presenter.updateDraft { it.copy(quantity = "99") }
            runCurrent()
            val pending = assertIs<ReceiptSubmission.Uncertain>(presenter.state.value.submission)
            assertEquals(20U, pending.attempt.input.wholePacks)
            assertEquals(
                "20",
                presenter.state.value.draft
                    ?.quantity,
            )
            assertEquals(1, ids)
            assertEquals(1, runtime.writes.size)
            presenter.checkOriginal()
            runCurrent()
            assertIs<ReceiptSubmission.Recorded>(presenter.state.value.submission)
            assertEquals(listOf(StockFixtures.operation), runtime.lookups)
            assertEquals(1, runtime.writes.size)
            presenter.finishRecorded()
            runCurrent()
            assertEquals(StockFixtures.received, presenter.state.value.page)
            assertNull(presenter.state.value.draft)
            presenter.close()
        }

    @Test
    fun missingRestoredReceiptNeverAuthorizesAnotherOperation() =
        runTest {
            val runtime = StockFake()
            val presenter = StockPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.beginReceipt()
            presenter.updateDraft(StockFixtures::readyDraft)
            presenter.submitReceipt(SnapshotRevision(7UL))
            runCurrent()
            runtime.stored = null
            presenter.checkOriginal()
            runCurrent()
            presenter.reviewRejected()
            presenter.closeDraft()
            presenter.beginReceipt()
            presenter.submitReceipt(SnapshotRevision(8UL))
            runCurrent()
            assertIs<ReceiptSubmission.Uncertain>(presenter.state.value.submission)
            assertEquals(1, runtime.writes.size)
            presenter.close()
        }

    @Test
    fun knownRejectionSurvivesAnAbsentReceiptReadAndKeepsItsInput() =
        runTest {
            val runtime = StockFake().apply { failure = stockConflict() }
            val presenter = StockPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.beginReceipt()
            presenter.updateDraft(StockFixtures::readyDraft)
            presenter.submitReceipt(SnapshotRevision(7UL))
            runCurrent()
            presenter.checkOriginal()
            runCurrent()
            assertIs<ReceiptSubmission.Rejected>(presenter.state.value.submission)
            presenter.reviewRejected()
            assertEquals(
                "20",
                presenter.state.value.draft
                    ?.quantity,
            )
            assertTrue(presenter.state.value.needsReview)
            presenter.close()
        }

    @Test
    fun staleDraftAdoptsNewVersionsOnlyAfterExplicitReviewAndSurvivesOrdinaryNavigation() =
        runTest {
            val runtime = StockFake()
            val presenter = StockPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.beginReceipt()
            presenter.updateDraft(StockFixtures::readyDraft)
            assertTrue(presenter.state.value.receiptOpen)
            presenter.hideReceipt()
            assertFalse(presenter.state.value.receiptOpen)
            runtime.page = StockFixtures.empty.copy(stockRevision = 2UL, catalogRevision = 6UL)
            presenter.refresh()
            runCurrent()
            presenter.beginReceipt()
            assertEquals(
                "20",
                presenter.state.value.draft
                    ?.quantity,
            )
            assertEquals(
                1UL,
                presenter.state.value.draft
                    ?.stockRevision,
            )
            assertTrue(presenter.state.value.needsReview)
            presenter.submitReceipt(SnapshotRevision(7UL))
            runCurrent()
            assertTrue(runtime.writes.isEmpty())
            presenter.reviewContext()
            runCurrent()
            assertEquals(
                1UL,
                presenter.state.value.draft
                    ?.stockRevision,
            )
            presenter.acceptReview()
            assertEquals(
                2UL,
                presenter.state.value.draft
                    ?.stockRevision,
            )
            assertEquals(
                6UL,
                presenter.state.value.draft
                    ?.catalogRevision,
            )
            assertEquals(
                "20",
                presenter.state.value.draft
                    ?.quantity,
            )
            presenter.close()
        }

    @Test
    fun nextPageUsesLoadedSearchAndBothNativeVersions() =
        runTest {
            val runtime = StockFake().apply { page = StockFixtures.received.copy(more = true, total = 2U) }
            val presenter = StockPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.search("apples")
            presenter.refresh()
            runCurrent()
            presenter.search("unsubmitted")
            presenter.nextPage()
            runCurrent()
            assertEquals(
                HubStockQuery(
                    expectedCatalogRevision = 5UL,
                    expectedStockRevision = 2UL,
                    offset = 1U,
                    text = "apples",
                ),
                runtime.queries.last(),
            )
            presenter.close()
        }

    @Test
    fun lateUncancelableReadCannotRestoreClosedIdentityData() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : StockFake() {
                    override suspend fun stock(query: HubStockQuery): HubStockPage =
                        withContext(NonCancellable) {
                            gate.await()
                            StockFixtures.received
                        }
                }
            val presenter = StockPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.close()
            gate.complete(Unit)
            runCurrent()
            assertEquals(StockViewState(), presenter.state.value)
        }

    @Test
    fun incompleteReferenceCatalogCannotEnableReceiptEntry() =
        runTest {
            val runtime =
                object : StockFake() {
                    override suspend fun references(query: HubReferenceQuery) =
                        HubReferencePage(StockFixtures.basis, 5UL, query.offset, 128U, false, emptyList())
                }
            val presenter = StockPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.beginReceipt()
            assertNull(presenter.state.value.page)
            assertNull(presenter.state.value.draft)
            presenter.close()
        }
}

private open class StockFake :
    HarvestCircleStockRuntime,
    HarvestCircleReferenceRuntime {
    var page = StockFixtures.empty
    var stored: HubStockReceipt? = null
    var failure: Exception? = null
    val queries = mutableListOf<HubStockQuery>()
    val writes = mutableListOf<Pair<RequestContext, HubReceiptInput>>()
    val lookups = mutableListOf<OperationId>()

    override suspend fun stock(query: HubStockQuery): HubStockPage {
        queries += query
        return page
    }

    override suspend fun receiveStock(
        context: RequestContext,
        input: HubReceiptInput,
    ): HubStockReceipt {
        writes += context to input
        failure?.let { throw it }
        stored = StockFixtures.receipt(context.operationId)
        page = StockFixtures.received
        error("Reply lost after native commit")
    }

    override suspend fun receiptOperation(operation: OperationId): HubStockReceipt? {
        lookups += operation
        return stored
    }

    override suspend fun references(query: HubReferenceQuery) =
        HubReferencePage(
            StockFixtures.basis,
            requireNotNull(query.expectedCatalogRevision),
            0U,
            4U,
            false,
            StockFixtures.references,
        )

    override suspend fun editReference(
        context: RequestContext,
        edit: HubReferenceEdit,
    ): HubReferenceReceipt = error("Unexpected edit")

    override suspend fun referenceOperation(operation: OperationId): HubReferenceReceipt? = error("Unexpected lookup")

    override suspend fun referenceHistory(
        id: String,
        afterRevision: ULong,
        limit: UInt,
    ): List<HubReferenceReceipt> = error("Unexpected history")
}

private fun stockConflict() =
    ApplicationFailure(
        ApplicationProblem(
            ApplicationErrorCode.HubConflict,
            ApplicationErrorCategory.Conflict,
            false,
            RecoveryAction.None,
            StockFixtures.operation,
            "Stock context changed",
        ),
    )
