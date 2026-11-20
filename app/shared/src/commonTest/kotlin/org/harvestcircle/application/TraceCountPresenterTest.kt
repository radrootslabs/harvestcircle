package org.harvestcircle.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class TraceCountPresenterTest {
    @Test
    fun everyCountReadSectionPagesSixtyFiveRowsAndRefreshResetsWithoutChangingNativeMeaning() =
        runTest {
            val original = traceCountSnapshot(65)
            val runtime =
                object : TraceFake() {
                    override suspend fun traceSnapshot(query: HubTraceSnapshotQuery) = original
                }
            val presenter = TracePresenter(runtime, this)
            presenter.attach("hub")
            presenter.openReceipt(original.trace.id)
            runCurrent()
            assertEquals(original, presenter.state.value.snapshot)
            for (section in TraceCountSection.entries) {
                presenter.countInterpretationsPage(section, -1)
                assertEquals(0, offset(presenter.state.value, section))
                presenter.countInterpretationsPage(section, 1)
                assertEquals(64, offset(presenter.state.value, section))
                presenter.countInterpretationsPage(section, 1)
                presenter.countInterpretationsPage(section, Int.MAX_VALUE)
                assertEquals(64, offset(presenter.state.value, section))
            }
            assertEquals(original, presenter.state.value.snapshot)
            presenter.refresh()
            runCurrent()
            for (section in TraceCountSection.entries) assertEquals(0, offset(presenter.state.value, section))
            assertEquals(original, presenter.state.value.snapshot)
            presenter.close()
        }

    @Test
    fun excessRowsCombinedClassificationBudgetAndWrongCutFailBeforeHistory() =
        runTest {
            val small = traceCountSnapshot(1)
            val counts = assertNotNull(small.countInterpretations)
            val many = traceCountSnapshot(4097)
            for (original in listOf(
                many,
                small.copy(countReturnAssociations = many.countReturnAssociations),
                small.copy(countInterpretations = counts.copy(stockRevision = small.stockRevision + 1UL)),
                small.copy(
                    countInterpretations =
                        counts.copy(
                            accounts =
                                listOf(
                                    counts.accounts.single().copy(
                                        classifications =
                                            List(4096) {
                                                counts.accounts
                                                    .single()
                                                    .classifications
                                                    .single()
                                            },
                                    ),
                                ),
                        ),
                ),
            )) {
                val runtime =
                    object : TraceFake() {
                        override suspend fun traceSnapshot(query: HubTraceSnapshotQuery) = original
                    }
                val presenter = TracePresenter(runtime, this)
                presenter.attach("hub")
                presenter.openReceipt(original.trace.id)
                runCurrent()
                assertNull(presenter.state.value.snapshot)
                assertNotNull(presenter.state.value.message)
                assertTrue(runtime.historyQueries.isEmpty())
                presenter.close()
            }
        }

    private fun offset(
        state: TraceViewState,
        section: TraceCountSection,
    ) = when (section) {
        TraceCountSection.Accounts -> state.countAccountsOffset
        TraceCountSection.Explanations -> state.countExplanationsOffset
        TraceCountSection.Classifications -> state.countClassificationsOffset
        TraceCountSection.ReturnAssociations -> state.countAssociationsOffset
    }
}
