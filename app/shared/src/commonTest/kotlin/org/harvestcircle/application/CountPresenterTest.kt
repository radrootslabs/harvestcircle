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
class CountPresenterTest {
    @Test
    fun observationKeepsInputAndChecksLostReplyWithoutAnotherSubmission() =
        runTest {
            val runtime = CountFake()
            runtime.loseCountReply = true
            val presenter =
                CountPresenter(
                    runtime,
                    runtime,
                    runtime,
                    runtime,
                    this,
                    OperationIdSource { StockFixtures.operation },
                    { "1970-01-01T00:01:40Z" },
                )
            presenter.attach("hub")
            presenter.beginObservation(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.edit { it.copy(quantity = "18", observation = "Observed eighteen scoped packs") }
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(1UL))
            runCurrent()
            assertIs<CountSubmission.Uncertain>(presenter.state.value.submission)
            assertEquals(
                18U,
                runtime.countWrites
                    .single()
                    .second.countedWholePacks,
            )
            presenter.checkOriginal()
            runCurrent()
            val recorded = assertIs<CountSubmission.Recorded>(presenter.state.value.submission)
            assertIs<CountResult.Observed>(recorded.result)
            assertEquals(1, runtime.countWrites.size)
            assertEquals(listOf(StockFixtures.operation), runtime.countLookups)
            presenter.reviewRecordedObservation()
            runCurrent()
            assertIs<CountContext.Review>(
                presenter.state.value.draft
                    ?.context,
            )
            assertFalse(assertNotNull(presenter.state.value.draft).fields.confirmed)
            presenter.close()
        }

    @Test
    fun parkedOpeningRequiresExplicitCurrentContextReviewAndRetainsItsFields() =
        runTest {
            val runtime = CountFake()
            val presenter =
                CountPresenter(
                    runtime,
                    runtime,
                    runtime,
                    runtime,
                    this,
                    OperationIdSource { StockFixtures.operation },
                    { "1970-01-01T00:01:40Z" },
                )
            presenter.attach("hub")
            presenter.beginOpening()
            runCurrent()
            presenter.edit { it.copy(quantity = "8", physicalLabel = "Opening eight", historyReference = "Reported external note") }
            presenter.confirm(true)
            presenter.park()
            assertFalse(presenter.state.value.open)
            runtime.stockPage = runtime.stockPage.copy(stockRevision = 3UL)
            presenter.resume()
            runCurrent()
            assertTrue(presenter.state.value.needsReview)
            assertEquals(
                "8",
                presenter.state.value.draft
                    ?.fields
                    ?.quantity,
            )
            assertFalse(assertNotNull(presenter.state.value.draft).fields.confirmed)
            assertEquals(
                3UL,
                presenter.state.value.review
                    ?.stockRevision,
            )
            presenter.acceptReview()
            assertFalse(presenter.state.value.needsReview)
            assertEquals(
                3UL,
                presenter.state.value.draft
                    ?.context
                    ?.stockRevision,
            )
            assertEquals(
                "Reported external note",
                presenter.state.value.draft
                    ?.fields
                    ?.historyReference,
            )
            presenter.close()
        }

    @Test
    fun nativeBlockedReviewCannotBeConfirmedOrPostedAndMissingOriginalStaysUncertain() =
        runTest {
            val runtime = CountFake()
            runtime.countReviewValue =
                runtime.countReviewValue.copy(ready = false, blockers = listOf(HubCountBlockReason.UncertainPriorOutcome))
            val presenter = CountPresenter(runtime, runtime, runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            presenter.beginReview(runtime.countReviewValue.observation.id)
            runCurrent()
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(1UL))
            runCurrent()
            assertFalse(assertNotNull(presenter.state.value.draft).fields.confirmed)
            assertEquals(0, runtime.posts)
            presenter.discard()
            runtime.loseCountReply = true
            presenter.beginObservation(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.edit { it.copy(quantity = "18", cutoffKind = HubOccurrenceKind.Unknown, observation = "Unknown cutoff is retained") }
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(1UL))
            runCurrent()
            runtime.countStored = null
            presenter.checkOriginal()
            runCurrent()
            assertIs<CountSubmission.Uncertain>(presenter.state.value.submission)
            presenter.discard()
            assertNotNull(presenter.state.value.submission)
            assertEquals(1, runtime.countWrites.size)
            presenter.close()
        }

    @Test
    fun noncancellableOldContextCannotPublishIntoAnotherOwnerOrRestoreOldInput() =
        runTest {
            val release = CompletableDeferred<Unit>()
            val runtime =
                object : CountFake() {
                    override suspend fun group(id: String): HubGroupView =
                        withContext(NonCancellable) {
                            release.await()
                            super.group(id)
                        }
                }
            val presenter = CountPresenter(runtime, runtime, runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("old")
            presenter.beginObservation(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.attach("new")
            release.complete(Unit)
            runCurrent()
            assertNull(presenter.state.value.draft)
            assertNull(presenter.state.value.target)
            assertFalse(presenter.state.value.loading)
            presenter.close()
        }
}

private open class CountFake :
    HandlingFake(),
    HarvestCircleStockRuntime,
    HarvestCircleCountRuntime {
    var stockPage = StockFixtures.received
    var loseCountReply = false
    var countStored: HubCountObservation? = null
    val countWrites = mutableListOf<Pair<RequestContext, HubCountObserveInput>>()
    val countLookups = mutableListOf<OperationId>()
    var posts = 0
    var countReviewValue =
        HubCountReview(
            observation(StockFixtures.operation, 18U, HubOccurrence(HubOccurrenceKind.Known, 100L)),
            3UL,
            5UL,
            HandlingFixtures.group.group,
            HubCountDifference.Shortage(2U),
            emptyList(),
            true,
        )

    override suspend fun stock(query: HubStockQuery) = stockPage

    override suspend fun receiveStock(
        context: RequestContext,
        input: HubReceiptInput,
    ): HubStockReceipt = error("Not receiving")

    override suspend fun receiptOperation(operation: OperationId): HubStockReceipt? = error("Not a receipt lookup")

    override suspend fun recordOpening(
        context: RequestContext,
        input: HubOpeningInput,
    ): HubOpeningReceipt = error("Opening not submitted by these tests")

    override suspend fun openingOperation(operation: OperationId): HubOpeningReceipt? = null

    override suspend fun observeCount(
        context: RequestContext,
        input: HubCountObserveInput,
    ): HubCountObservation {
        countWrites += context to input
        val result = observation(context.operationId, input.countedWholePacks, input.cutoff)
        countStored = result
        if (loseCountReply) error("Reply lost after native recording")
        return result
    }

    override suspend fun countObservationOperation(operation: OperationId): HubCountObservation? {
        countLookups += operation
        return countStored
    }

    override suspend fun countReview(countId: String) = countReviewValue

    override suspend fun postCount(
        context: RequestContext,
        input: HubCountPostInput,
    ): HubCountPostReceipt {
        posts += 1
        error("Blocked review must not post")
    }

    override suspend fun countPostOperation(operation: OperationId): HubCountPostReceipt? = null

    private fun observation(
        operation: OperationId,
        packs: UInt,
        cutoff: HubOccurrence,
    ) = HubCountObservation(
        "09".repeat(16),
        operation,
        3UL,
        "01".repeat(16),
        2UL,
        5UL,
        StockFixtures.trace.id,
        HandlingFixtures.group.group.id,
        1UL,
        StockFixtures.product,
        StockFixtures.pack,
        StockFixtures.receiving,
        20U,
        packs,
        UnixSeconds(100),
        null,
        cutoff,
        StockFixtures.actor,
        UnixSeconds(100),
        "Observed fictional packs",
    )
}
