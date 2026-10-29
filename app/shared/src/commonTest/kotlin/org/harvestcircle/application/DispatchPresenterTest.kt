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
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class DispatchPresenterTest {
    @Test
    fun typingDuringSourceReadKeepsLatestObservationAndRecipient() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : DispatchFake() {
                    override suspend fun group(id: String): HubGroupView {
                        gate.await()
                        return super.group(id)
                    }
                }
            val presenter = DispatchPresenter(runtime, runtime, runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.begin(HubDispatchKind.Prepare)
            runCurrent()
            presenter.selectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            assertTrue(presenter.state.value.loading)
            presenter.edit(runtime.recipient.id, "Latest observation typed during native reading")
            gate.complete(Unit)
            runCurrent()
            assertEquals(
                "Latest observation typed during native reading",
                presenter.state.value.draft
                    ?.observation,
            )
            assertEquals(
                runtime.recipient.id,
                presenter.state.value.draft
                    ?.recipientId,
            )
            assertEquals(
                1,
                presenter.state.value.draft
                    ?.lines
                    ?.size,
            )
            presenter.close()
        }

    @Test
    fun issueReviewShowsCurrentPhysicalFactsWithoutRewritingSavedPreparation() =
        runTest {
            val runtime = DispatchFake()
            val presenter = DispatchPresenter(runtime, runtime, runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.inspect(runtime.receipt.record.id)
            runCurrent()
            presenter.begin(HubDispatchKind.Issue)
            runCurrent()
            runtime.stockPage = runtime.stockPage.copy(stockRevision = 4UL)
            runtime.groupView =
                runtime.groupView.copy(stockRevision = 4UL, group = runtime.groupView.group.copy(wholePacks = 18U, revision = 2UL))
            runtime.page = runtime.page.copy(stockRevision = 4UL)
            presenter.reviewContext()
            runCurrent()
            val review = requireNotNull(presenter.state.value.review)
            assertEquals(
                18U,
                review.draft.lines
                    .single()
                    .wholePacks,
            )
            assertEquals(
                20U,
                review.draft.target
                    ?.draft
                    ?.wholePacks,
            )
            presenter.acceptReview()
            val input =
                assertIs<HubDispatchInput.Issue>(
                    presenter.state.value.draft
                        ?.input(),
                )
            assertEquals(runtime.receipt.record.id, input.value.preparationId)
            assertEquals(4UL, input.value.expectedStockRevision)
            assertTrue(runtime.dispatchWrites.isEmpty())
            presenter.close()
        }

    @Test
    fun retainedDraftAndUncertainIntentNeverAllocateAnotherOperation() =
        runTest {
            val runtime = DispatchFake()
            var ids = 0
            val presenter =
                DispatchPresenter(
                    runtime,
                    runtime,
                    runtime,
                    runtime,
                    this,
                    OperationIdSource {
                        ids++
                        StockFixtures.operation
                    },
                )
            presenter.attach("hub")
            runCurrent()
            presenter.begin(HubDispatchKind.Prepare)
            runCurrent()
            presenter.edit(runtime.recipient.id, "Reviewed fictional twenty")
            presenter.selectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.park()
            presenter.refresh()
            runCurrent()
            presenter.begin(HubDispatchKind.Prepare)
            assertEquals(
                "Reviewed fictional twenty",
                presenter.state.value.draft
                    ?.observation,
            )
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            val frozen = assertIs<DispatchSubmission.Uncertain>(presenter.state.value.submission)
            presenter.edit(null, "Changed")
            presenter.discard()
            presenter.submit(SnapshotRevision(8UL))
            presenter.begin(HubDispatchKind.Prepare)
            presenter.checkOriginal()
            runCurrent()
            assertIs<DispatchSubmission.Uncertain>(presenter.state.value.submission)
            assertEquals(1, ids)
            assertEquals(1, runtime.dispatchWrites.size)
            assertEquals(frozen.attempt.input, runtime.dispatchWrites.single().second)
            runtime.original = runtime.receipt
            presenter.checkOriginal()
            runCurrent()
            assertEquals(runtime.receipt, assertIs<DispatchSubmission.Recorded>(presenter.state.value.submission).receipt)
            assertEquals(1, ids)
            presenter.close()
        }

    @Test
    fun explicitContextReviewRetainsChoicesAndShowsNativeChangedMaterialBeforeAcceptance() =
        runTest {
            val runtime = DispatchFake()
            val presenter = DispatchPresenter(runtime, runtime, runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.begin(HubDispatchKind.Prepare)
            runCurrent()
            presenter.edit(runtime.recipient.id, "Retain this observation")
            presenter.selectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            runtime.stockPage = runtime.stockPage.copy(stockRevision = 3UL)
            runtime.groupView =
                runtime.groupView.copy(stockRevision = 3UL, group = runtime.groupView.group.copy(wholePacks = 18U, revision = 2UL))
            runtime.page = runtime.page.copy(stockRevision = 3UL)
            presenter.refresh()
            runCurrent()
            assertTrue(presenter.state.value.needsReview)
            presenter.submit(SnapshotRevision(7UL))
            assertTrue(runtime.dispatchWrites.isEmpty())
            presenter.reviewContext()
            runCurrent()
            assertEquals(
                20U,
                presenter.state.value.draft
                    ?.lines
                    ?.single()
                    ?.wholePacks,
            )
            assertEquals(
                18U,
                presenter.state.value.review
                    ?.draft
                    ?.lines
                    ?.single()
                    ?.wholePacks,
            )
            assertEquals(
                "Retain this observation",
                presenter.state.value.review
                    ?.draft
                    ?.observation,
            )
            presenter.acceptReview()
            assertFalse(presenter.state.value.needsReview)
            assertEquals(
                18U,
                presenter.state.value.draft
                    ?.lines
                    ?.single()
                    ?.wholePacks,
            )
            assertEquals(
                runtime.recipient.id,
                presenter.state.value.draft
                    ?.recipientId,
            )
            presenter.close()
        }

    @Test
    fun staleNativeSelectionCannotSilentlyRefreshDraftAndLateReadsCannotCrossHubs() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : DispatchFake() {
                    var delay = false

                    override suspend fun dispatchPage(query: HubDispatchQuery): HubDispatchPage =
                        withContext(NonCancellable) {
                            if (delay) gate.await()
                            super.dispatchPage(query)
                        }
                }
            val presenter = DispatchPresenter(runtime, runtime, runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("old")
            runCurrent()
            presenter.begin(HubDispatchKind.Prepare)
            runCurrent()
            runtime.groupView = runtime.groupView.copy(stockRevision = 3UL)
            presenter.selectGroup(runtime.groupView.group.id)
            runCurrent()
            assertTrue(requireNotNull(presenter.state.value.draft).lines.isEmpty())
            assertTrue(presenter.state.value.needsReview)
            assertNotNull(presenter.state.value.message)
            runtime.delay = true
            presenter.refresh()
            runCurrent()
            presenter.attach("new")
            runCurrent()
            gate.complete(Unit)
            runCurrent()
            assertNull(presenter.state.value.draft)
            assertNull(presenter.state.value.submission)
            presenter.close()
        }
}

internal open class DispatchFake :
    HandlingFake(),
    HarvestCircleDispatchRuntime,
    HarvestCircleStockRuntime {
    val recipient = HubReferenceRecord("08".repeat(16), 1UL, HubReferenceValue(HubReferenceKind.Party, "Fictional recipient A"))
    var stockPage = StockFixtures.received
    var page = HubDispatchPage(2UL, 5UL, 0U, 0U, false, emptyList())
    val dispatchWrites = mutableListOf<Pair<RequestContext, HubDispatchInput>>()
    var original: HubDispatchReceipt? = null
    private val stamp = HubDispatchStamp(StockFixtures.operation, 3UL, StockFixtures.actor, UnixSeconds(100), "Reviewed sample material")
    val receipt =
        HubDispatchReceipt(
            StockFixtures.operation,
            HubDispatchKind.Prepare,
            5UL,
            HubDispatchRecord(
                "09".repeat(16),
                "01".repeat(16),
                1UL,
                HubDispatchState.Draft,
                stamp,
                stamp,
                HubDispatchMaterial(
                    2UL,
                    5UL,
                    StockFixtures.basis,
                    recipient,
                    listOf(HandlingFixtures.group.dispatchLine()),
                    20U,
                    stamp.observation,
                ),
                emptyList(),
            ),
        )

    override suspend fun references(query: HubReferenceQuery) =
        HubReferencePage(
            StockFixtures.basis,
            5UL,
            0U,
            5U,
            false,
            StockFixtures.references + recipient,
        )

    override suspend fun dispatchPage(query: HubDispatchQuery) = page

    override suspend fun dispatchRecord(id: String) =
        HubDispatchView(page.stockRevision, page.catalogRevision, receipt.record, receipt.record.state, null)

    override suspend fun dispatchOperation(operation: OperationId) = original

    override suspend fun applyDispatch(
        context: RequestContext,
        input: HubDispatchInput,
    ): HubDispatchReceipt {
        dispatchWrites += context to input
        throw IllegalStateException("Lost native reply")
    }

    override suspend fun stock(query: HubStockQuery) = stockPage

    override suspend fun receiveStock(
        context: RequestContext,
        input: HubReceiptInput,
    ): HubStockReceipt = error("Not used")

    override suspend fun receiptOperation(operation: OperationId): HubStockReceipt? = null
}
