package org.harvestcircle.ui.shell

import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlinx.coroutines.CompletableDeferred
import org.harvestcircle.application.HandlingFake
import org.harvestcircle.application.HandlingFixtures
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HandlingSubmission
import org.harvestcircle.application.HubHandlingQuery
import org.harvestcircle.application.HubWorkPage
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.application.StockFixtures
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class HandlingWorkspaceTest {
    @Test
    fun uncertainMovementOffersOriginalLookupWithoutAnotherSubmission() =
        runComposeUiTest {
            val runtime = HandlingFake()
            lateinit var presenter: HandlingPresenter
            setHarvestCircleContent {
                val scope = rememberCoroutineScope()
                presenter = remember { HandlingPresenter(runtime, runtime, scope, OperationIdSource { StockFixtures.operation }) }
                val state by presenter.state.collectAsState()
                LaunchedEffect(Unit) { presenter.attach("hub") }
                HandlingWorkspace(state, presenter, { presenter.submit(SnapshotRevision(7UL)) }, {}, {}, {}, {})
            }
            waitUntil { presenter.state.value.work != null }
            runOnIdle { presenter.inspectGroup(HandlingFixtures.group.group.id) }
            waitUntil { presenter.state.value.group != null }
            onNodeWithTag("handling-begin-move").performScrollTo().performClick()
            onNodeWithTag("handling-target").performScrollTo().performClick()
            onNodeWithTag("handling-target-${StockFixtures.planned.id}").performScrollTo().performClick()
            onNodeWithTag("handling-observation").performScrollTo().performTextInput("Observed whole-group arrival")
            onNodeWithTag("handling-save").performScrollTo().performClick()
            waitUntil { presenter.state.value.submission is HandlingSubmission.Uncertain }
            onNodeWithTag("handling-save").assertDoesNotExist()
            onNodeWithTag("handling-discard").assertDoesNotExist()
            onNodeWithTag("handling-target").assertDoesNotExist()
            runOnIdle { runtime.stored = null }
            onNodeWithTag("handling-check-original").performScrollTo().performClick()
            waitUntil { !presenter.state.value.loading }
            onNodeWithTag("handling-check-original").performScrollTo().assertIsEnabled()
            assertEquals(1, runtime.writes.size)
            assertEquals(listOf(StockFixtures.operation), runtime.lookups)
            runOnIdle { presenter.close() }
        }

    @Test
    fun loadingClearsStaleRowsAndDisablesTabActionsUntilTheReadCompletes() =
        runComposeUiTest {
            val gate = CompletableDeferred<Unit>()
            var reads = 0
            val runtime =
                object : HandlingFake() {
                    override suspend fun work(query: HubHandlingQuery): HubWorkPage {
                        reads += 1
                        if (reads > 1) gate.await()
                        return HandlingFixtures.page
                    }
                }
            lateinit var presenter: HandlingPresenter
            setHarvestCircleContent {
                val scope = rememberCoroutineScope()
                presenter = remember { HandlingPresenter(runtime, runtime, scope, OperationIdSource { StockFixtures.operation }) }
                val state by presenter.state.collectAsState()
                LaunchedEffect(Unit) { presenter.attach("hub") }
                HandlingWorkspace(state, presenter, {}, {}, {}, {}, {})
            }
            waitUntil { presenter.state.value.work != null }
            runOnIdle { presenter.refresh() }
            waitUntil { presenter.state.value.loading }
            onNodeWithTag("handling-work-tab").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("handling-discrepancies-tab").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("work-inspect-${HandlingFixtures.requirement.id}").assertDoesNotExist()
            runOnIdle { gate.complete(Unit) }
            waitUntil { !presenter.state.value.loading }
            onNodeWithTag("handling-discrepancies-tab").performScrollTo().assertIsEnabled()
            runOnIdle { presenter.close() }
        }
}
