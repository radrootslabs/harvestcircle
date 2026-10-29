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
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class TracePresenterTest {
    @Test
    fun dismissalUsesShellBackAndEscapeWhileModalPriorityAndPrimaryNavigationRemainIntact() =
        runTest {
            val presenter = TracePresenter(TraceFake(), this)
            presenter.attach("hub")
            val back = HarvestCircleShellIntent.Navigation(org.harvestcircle.navigation.NavigationIntent.Back)
            val escape = HarvestCircleShellIntent.Overlay(OverlayIntent.Escape(null))
            assertFalse(presenter.intercept(back, null))
            presenter.browse()
            runCurrent()
            assertFalse(presenter.intercept(escape, FoundationOverlay.Status(StatusOverlayKey.Signer)))
            assertTrue(presenter.state.value.open)
            assertTrue(presenter.intercept(back, null))
            assertFalse(presenter.state.value.open)
            presenter.browse()
            runCurrent()
            assertTrue(presenter.intercept(escape, null))
            assertFalse(presenter.state.value.open)
            presenter.browse()
            runCurrent()
            assertFalse(presenter.intercept(HarvestCircleShellIntent.Navigate(org.harvestcircle.product.ScreenKey.HubWork), null))
            assertFalse(presenter.state.value.open)
            presenter.close()
        }

    @Test
    fun snapshotAndHistoryPublishOnlyAsOnePinnedPopulationContext() =
        runTest {
            val runtime = TraceFake()
            val presenter = TracePresenter(runtime, this)
            presenter.attach("hub")
            presenter.openReceipt(StockFixtures.trace.id)
            runCurrent()
            val state = presenter.state.value
            assertEquals(runtime.snapshot, state.snapshot)
            assertEquals(runtime.history, state.history)
            assertEquals(HubTraceHistoryQuery(StockFixtures.trace.id, 2UL, 5UL), runtime.historyQueries.single())
            presenter.chooseSection(TraceSection.History)
            assertEquals(TraceSection.History, presenter.state.value.section)
            presenter.park()
            assertFalse(presenter.state.value.open)
            presenter.close()
        }

    @Test
    fun changedOrMismatchedHistoryNeverPublishesMixedFactsAndRetainsExactTargetForRefresh() =
        runTest {
            val runtime = TraceFake()
            runtime.history = runtime.history.copy(stockRevision = 3UL)
            val presenter = TracePresenter(runtime, this)
            presenter.attach("hub")
            presenter.openReceipt(StockFixtures.trace.id)
            runCurrent()
            assertNull(presenter.state.value.snapshot)
            assertNull(presenter.state.value.history)
            assertTrue(presenter.state.value.needsRefresh)
            assertEquals(StockFixtures.trace.id, presenter.state.value.targetId)
            assertNotNull(presenter.state.value.message)
            runtime.history = runtime.history.copy(stockRevision = 2UL)
            presenter.refresh()
            runCurrent()
            assertEquals(runtime.snapshot, presenter.state.value.snapshot)
            assertFalse(presenter.state.value.needsRefresh)
            presenter.close()
        }

    @Test
    fun lateReadCannotRetargetSelectionOrCrossHubAttachment() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : TraceFake() {
                    override suspend fun traceSnapshot(query: HubTraceSnapshotQuery): HubTraceSnapshot =
                        withContext(NonCancellable) {
                            gate.await()
                            super.traceSnapshot(query)
                        }
                }
            val presenter = TracePresenter(runtime, this)
            presenter.attach("old")
            presenter.openReceipt(StockFixtures.trace.id)
            runCurrent()
            presenter.attach("new")
            presenter.browse()
            runCurrent()
            gate.complete(Unit)
            runCurrent()
            assertNull(presenter.state.value.targetId)
            assertNull(presenter.state.value.snapshot)
            assertEquals(
                runtime.page.rows,
                presenter.state.value.page
                    ?.rows,
            )
            assertFalse(presenter.state.value.loading)
            presenter.close()
        }

    @Test
    fun sourceImpactPassesNativeScopeWithoutClientFilteringAndPreservesAllReturnedRows() =
        runTest {
            val runtime = TraceFake()
            val presenter = TracePresenter(runtime, this)
            presenter.attach("hub")
            presenter.browse(HubTraceScope.SourceImpact, "08".repeat(16))
            presenter.search("A narrower label must not filter this impact scope")
            runCurrent()
            assertEquals(HubTraceSelection(HubTraceScope.SourceImpact, sourceId = "08".repeat(16)), runtime.queries.single().selection)
            assertEquals("", presenter.state.value.queryText)
            assertEquals(
                runtime.page.rows,
                presenter.state.value.page
                    ?.rows,
            )
            presenter.close()
        }

    @Test
    fun historyContinuationKeepsSnapshotPinsAndNeverReplacesThePopulationOnFailure() =
        runTest {
            val runtime = TraceFake()
            val rows = List(64) { runtime.history.rows.single() }
            runtime.history = runtime.history.copy(rows = rows, total = 65U, more = true)
            val presenter = TracePresenter(runtime, this)
            presenter.attach("hub")
            presenter.openReceipt(StockFixtures.trace.id)
            runCurrent()
            runtime.failHistory = true
            presenter.historyNext()
            runCurrent()
            assertEquals(HubTraceHistoryQuery(StockFixtures.trace.id, 2UL, 5UL, 64U), runtime.historyQueries.last())
            assertEquals(runtime.snapshot, presenter.state.value.snapshot)
            assertNull(presenter.state.value.history)
            assertTrue(presenter.state.value.needsRefresh)
            assertEquals(StockFixtures.trace.id, presenter.state.value.targetId)
            presenter.close()
        }
}

internal open class TraceFake : HarvestCircleTraceRuntime {
    val page = HubTracePage(2UL, 5UL, HubTraceSelection(), 0U, 1U, false, listOf(HubTraceRow(StockFixtures.trace, 20U, 0U)))
    val snapshot =
        HubTraceSnapshot(
            "01".repeat(16),
            2UL,
            5UL,
            StockFixtures.trace,
            20U,
            0U,
            listOf(
                HubTracePosition(
                    StockFixtures.position.groupId,
                    1UL,
                    20U,
                    StockFixtures.receiving,
                    null,
                    HubAcceptanceStatus.ReviewRequired,
                    null,
                    emptyList(),
                ),
            ),
            emptyList(),
        )
    var history =
        HubTraceHistoryPage(
            StockFixtures.trace.id,
            2UL,
            5UL,
            0U,
            1U,
            false,
            listOf(
                HubTraceHistoryEntry(
                    StockFixtures.operation,
                    2UL,
                    HubTraceHistoryKind.Receipt,
                    StockFixtures.actor,
                    UnixSeconds(100),
                    HubOccurrence(HubOccurrenceKind.Unknown),
                    StockFixtures.trace.observation,
                    StockFixtures.trace.id,
                ),
            ),
        )
    val queries = mutableListOf<HubTraceQuery>()
    val historyQueries = mutableListOf<HubTraceHistoryQuery>()
    var failHistory = false

    override suspend fun tracePage(query: HubTraceQuery): HubTracePage {
        queries += query
        return page.copy(selection = query.selection, offset = query.offset)
    }

    override suspend fun traceSnapshot(query: HubTraceSnapshotQuery): HubTraceSnapshot = snapshot

    override suspend fun traceHistory(query: HubTraceHistoryQuery): HubTraceHistoryPage {
        historyQueries += query
        check(!failHistory) { "History changed while continuing the captured snapshot" }
        return history.copy(offset = query.offset)
    }

    override suspend fun localOutput(request: HubLocalOutputRequest): HubLocalOutput =
        error("Read inspection must not prepare or submit output")
}
