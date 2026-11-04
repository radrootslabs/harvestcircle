package org.harvestcircle.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull

@OptIn(ExperimentalCoroutinesApi::class)
class TraceRecipientPresenterTest {
    @Test
    fun recipientHistoryAndAffectedScopePageIndependentlyAndRefreshDropsOldSelection() =
        runTest {
            val original = TraceFake().snapshot.copy(recipientCorrections = traceRecipientRows(65))
            val runtime =
                object : TraceFake() {
                    override suspend fun traceSnapshot(query: HubTraceSnapshotQuery) = original
                }
            val presenter = TracePresenter(runtime, this)
            presenter.attach("hub")
            presenter.openReceipt(StockFixtures.trace.id)
            runCurrent()
            presenter.recipientCorrectionsPage(-1)
            assertEquals(0, presenter.state.value.recipientCorrectionsOffset)
            presenter.recipientCorrectionsPage(1)
            presenter.recipientCorrectionsPage(2)
            assertEquals(64, presenter.state.value.recipientCorrectionsOffset)
            val selected = original.recipientCorrections.first().operationId
            presenter.recipientImpact(selected)
            presenter.recipientImpactPage(1)
            presenter.recipientImpactPage(1)
            assertEquals(selected, presenter.state.value.recipientImpactOperationId)
            assertEquals(64, presenter.state.value.recipientImpactOffset)
            presenter.recipientImpact(OperationId.from("01900000-0000-7000-8000-000000000999"))
            assertEquals(selected, presenter.state.value.recipientImpactOperationId)
            assertEquals(original, presenter.state.value.snapshot)
            presenter.refresh()
            runCurrent()
            assertEquals(0, presenter.state.value.recipientCorrectionsOffset)
            assertEquals(0, presenter.state.value.recipientImpactOffset)
            assertNull(presenter.state.value.recipientImpactOperationId)
            assertEquals(original, presenter.state.value.snapshot)
            presenter.close()
        }

    @Test
    fun excessiveRecipientHistoryOrImpactDoesNotPublishAPartialSnapshot() =
        runTest {
            val oversizedPopulation = traceRecipientRows(1, 1).map { it.copy(originalPopulationIds = List(4097) { "a".repeat(32) }) }
            val oversizedVersions = traceRecipientRows(1, 1).map { it.copy(originalIssuedVersionIds = List(4097) { "b".repeat(32) }) }
            for (corrections in listOf(traceRecipientRows(4097, 1), traceRecipientRows(1, 4097), oversizedPopulation, oversizedVersions)) {
                val runtime =
                    object : TraceFake() {
                        override suspend fun traceSnapshot(query: HubTraceSnapshotQuery) = snapshot.copy(recipientCorrections = corrections)
                    }
                val presenter = TracePresenter(runtime, this)
                presenter.attach("hub")
                presenter.openReceipt(StockFixtures.trace.id)
                runCurrent()
                assertNull(presenter.state.value.snapshot)
                assertNotNull(presenter.state.value.message)
                assertEquals(emptyList(), runtime.historyQueries)
                presenter.close()
            }
        }
}
