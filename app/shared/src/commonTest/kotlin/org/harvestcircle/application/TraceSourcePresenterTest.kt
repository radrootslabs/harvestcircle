package org.harvestcircle.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull

@OptIn(ExperimentalCoroutinesApi::class)
class TraceSourcePresenterTest {
    @Test
    fun sourceHistoryAndAffectedScopePageIndependentlyAndRefreshDropsOldSelection() =
        runTest {
            val original = TraceFake().snapshot.copy(sourceCorrections = traceSourceRows(65))
            val runtime =
                object : TraceFake() {
                    override suspend fun traceSnapshot(query: HubTraceSnapshotQuery) = original
                }
            val presenter = TracePresenter(runtime, this)
            presenter.attach("hub")
            presenter.openReceipt(StockFixtures.trace.id)
            runCurrent()
            presenter.sourceCorrectionsPage(-1)
            assertEquals(0, presenter.state.value.sourceCorrectionsOffset)
            presenter.sourceCorrectionsPage(1)
            presenter.sourceCorrectionsPage(2)
            assertEquals(64, presenter.state.value.sourceCorrectionsOffset)
            val selected = original.sourceCorrections.first().operationId
            presenter.sourceImpact(selected)
            presenter.sourceImpactPage(1)
            presenter.sourceImpactPage(1)
            assertEquals(selected, presenter.state.value.sourceImpactOperationId)
            assertEquals(64, presenter.state.value.sourceImpactOffset)
            presenter.sourceImpact(OperationId.from("01900000-0000-7000-8000-000000000999"))
            assertEquals(selected, presenter.state.value.sourceImpactOperationId)
            assertEquals(original, presenter.state.value.snapshot)
            presenter.refresh()
            runCurrent()
            assertEquals(0, presenter.state.value.sourceCorrectionsOffset)
            assertEquals(0, presenter.state.value.sourceImpactOffset)
            assertNull(presenter.state.value.sourceImpactOperationId)
            assertEquals(original, presenter.state.value.snapshot)
            presenter.close()
        }

    @Test
    fun excessiveSourceHistoryOrImpactDoesNotPublishAPartialSnapshot() =
        runTest {
            for ((rows, impacts) in listOf(4097 to 1, 1 to 4097)) {
                val runtime =
                    object : TraceFake() {
                        override suspend fun traceSnapshot(query: HubTraceSnapshotQuery) =
                            snapshot.copy(sourceCorrections = traceSourceRows(rows, impacts))
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
