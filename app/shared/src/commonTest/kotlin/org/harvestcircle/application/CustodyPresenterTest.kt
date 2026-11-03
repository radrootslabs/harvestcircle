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
class CustodyPresenterTest {
    @Test
    fun aFailedSecondRefreshCannotReenableAnOlderStagedReviewOrConfirmation() =
        runTest {
            val runtime = CustodyFake()
            val presenter = CustodyPresenter(runtime, runtime, runtime, runtime, null, this, OperationIdSource { error("No write") })
            presenter.attach("hub", binding(1UL))
            presenter.beginReturn()
            runCurrent()
            assertNotNull(presenter.state.value.draft)
            presenter.refresh()
            runCurrent()
            assertNotNull(presenter.state.value.review)
            runtime.failRead = true
            presenter.refresh()
            runCurrent()
            assertTrue(presenter.state.value.needsReview)
            assertNull(presenter.state.value.review)
            presenter.acceptReview()
            presenter.confirm(true)
            assertFalse(requireNotNull(presenter.state.value.draft).fields.common.confirmed)
            assertEquals(0, runtime.returnWrites)
            runtime.failRead = false
            presenter.refresh()
            runCurrent()
            presenter.acceptReview()
            presenter.confirm(true)
            assertTrue(requireNotNull(presenter.state.value.draft).fields.common.confirmed)
            assertFalse(presenter.state.value.needsReview)
            presenter.close()
        }

    @Test
    fun lostReplyAndMissingOriginalRetainTheSameAttemptAndCannotAllocateAnotherSubmission() =
        runTest {
            val runtime = CustodyFake()
            var ids = 0
            val presenter =
                CustodyPresenter(
                    runtime,
                    runtime,
                    runtime,
                    runtime,
                    null,
                    this,
                    OperationIdSource {
                        ids++
                        StockFixtures.operation
                    },
                )
            presenter.attach("hub", binding(1UL))
            presenter.beginReturn()
            runCurrent()
            presenter.edit {
                it.copy(
                    common =
                        it.common.copy(
                            variantId = StockFixtures.pack.id,
                            locationId = StockFixtures.receiving.id,
                            quantity = "2",
                            physicalLabel = "Returned two",
                            physicalObservation = "Observed actual separate crates",
                            observation = "Actual intake",
                        ),
                )
            }
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(1UL))
            runCurrent()
            val uncertain = assertIs<CustodySubmission.Uncertain>(presenter.state.value.submission)
            assertEquals(1, runtime.returnWrites)
            presenter.discard()
            presenter.beginDisposition(HandlingFixtures.group.group.id)
            presenter.beginOriginalLookup()
            presenter.checkOriginal()
            runCurrent()
            assertEquals(uncertain.attempt, assertIs<CustodySubmission.Uncertain>(presenter.state.value.submission).attempt)
            assertEquals(1, ids)
            assertEquals(1, runtime.returnWrites)
            assertEquals(listOf(StockFixtures.operation), runtime.originalLookups)
            assertNull(presenter.state.value.originalLookup)
            presenter.submit(SnapshotRevision(1UL))
            assertEquals(1, ids)
            presenter.close()
        }

    @Test
    fun oldNoncancellableReadCannotReopenAReplacedNativeBindingOrPublishOldFacts() =
        runTest {
            val release = CompletableDeferred<Unit>()
            val runtime =
                object : CustodyFake() {
                    override suspend fun stock(query: HubStockQuery): HubStockPage =
                        withContext(NonCancellable) {
                            release.await()
                            super.stock(query)
                        }
                }
            val presenter = CustodyPresenter(runtime, runtime, runtime, runtime, null, this, OperationIdSource { error("No write") })
            presenter.attach("hub", binding(1UL))
            presenter.beginReturn()
            runCurrent()
            presenter.attach("hub", binding(3UL))
            release.complete(Unit)
            runCurrent()
            presenter.beginReturn()
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(1UL))
            assertFalse(presenter.state.value.available)
            assertNull(presenter.state.value.draft)
            assertNull(presenter.state.value.target)
            assertEquals(0, runtime.returnWrites)
        }

    @Test
    fun parkingAndEditingClearConfirmationAndOriginalLookupNeverBuildsAnAttempt() =
        runTest {
            val runtime = CustodyFake()
            val presenter = CustodyPresenter(runtime, runtime, runtime, runtime, null, this, OperationIdSource { error("Read-only") })
            presenter.attach("hub", binding(1UL))
            presenter.beginReturn()
            runCurrent()
            presenter.confirm(true)
            presenter.edit { it.copy(common = it.common.copy(quantity = "2")) }
            assertFalse(requireNotNull(presenter.state.value.draft).fields.common.confirmed)
            presenter.confirm(true)
            presenter.park()
            assertFalse(requireNotNull(presenter.state.value.draft).fields.common.confirmed)
            presenter.resume()
            runCurrent()
            assertTrue(presenter.state.value.needsReview)
            presenter.discard()
            presenter.beginOriginalLookup()
            presenter.editOriginalLookup(CustodyLookupKind.Return, StockFixtures.operation.value)
            presenter.lookupOriginal()
            runCurrent()
            assertNull(presenter.state.value.submission)
            assertNull(presenter.state.value.draft)
            assertEquals(0, runtime.returnWrites)
            assertEquals(listOf(StockFixtures.operation), runtime.originalLookups)
            presenter.close()
        }

    private fun binding(generation: ULong) = NativeSessionBinding("01".repeat(16), generation)
}

private open class CustodyFake :
    HandlingFake(),
    HarvestCircleStockRuntime,
    HarvestCircleCustodyRuntime {
    var failRead = false
    var returnWrites = 0
    val originalLookups = mutableListOf<OperationId>()

    override suspend fun stock(query: HubStockQuery): HubStockPage {
        check(!failRead) { "Current native read failed" }
        return StockFixtures.received
    }

    override suspend fun receiveStock(
        context: RequestContext,
        input: HubReceiptInput,
    ): HubStockReceipt = error("No receipt")

    override suspend fun receiptOperation(operation: OperationId): HubStockReceipt? = null

    override suspend fun applyReturn(
        context: RequestContext,
        input: HubReturnInput,
    ): HubReturnReceipt {
        returnWrites++
        error("Unknown native reply")
    }

    override suspend fun returnOperation(operation: OperationId): HubReturnReceipt? {
        originalLookups += operation
        return null
    }

    override suspend fun applyDisposition(
        context: RequestContext,
        input: HubDispositionInput,
    ): HubDispositionReceipt = error("No new disposition")

    override suspend fun dispositionOperation(operation: OperationId): HubDispositionReceipt? = null
}
