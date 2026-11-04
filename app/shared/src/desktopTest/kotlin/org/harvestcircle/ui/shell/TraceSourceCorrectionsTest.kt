package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import org.harvestcircle.application.HubTraceQuery
import org.harvestcircle.application.HubTraceSnapshotQuery
import org.harvestcircle.application.StockFixtures
import org.harvestcircle.application.TraceFake
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.application.traceSourceRows
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class TraceSourceCorrectionsTest {
    @Test
    fun amendedSourceSearchResultShowsOriginalAndCurrentMeaningBeforeOpening() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val corrections = traceSourceRows(1)
            val base = TraceFake().page
            val result = base.copy(stockRevision = 3UL, rows = listOf(base.rows.single().copy(sourceCorrections = corrections)))
            val runtime =
                object : TraceFake() {
                    override suspend fun tracePage(query: HubTraceQuery) = result.copy(selection = query.selection)
                }
            var presenter: TracePresenter? = null
            setHarvestCircleContent {
                val scope = rememberCoroutineScope()
                val active =
                    androidx.compose.runtime.remember {
                        TracePresenter(runtime, scope).also {
                            presenter = it
                            it.attach("hub")
                            it.browse()
                        }
                    }
                val state by active.state.collectAsState()
                TraceWorkspace(state, active)
            }
            waitUntil { presenter?.state?.value?.page != null }
            onNodeWithText("Original source: Unknown source; no relationship is established").assertExists()
            onNodeWithText("Current source interpretation: ${traceSourceMeaning(corrections.single().after)}").assertExists()
            onNodeWithText("1 linked source amendments; original and amended scope retained").assertExists()
            runOnIdle { presenter?.close() }
        }

    @Test
    fun renderedSourceHistoryAndAffectedReferencesRemainPagedWithUnknownOriginalVisible() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val original = TraceFake().snapshot.copy(sourceCorrections = traceSourceRows(65))
            val runtime =
                object : TraceFake() {
                    override suspend fun traceSnapshot(query: HubTraceSnapshotQuery) = original
                }
            var presenter: TracePresenter? = null
            setHarvestCircleContent {
                val scope = rememberCoroutineScope()
                val active =
                    androidx.compose.runtime.remember {
                        TracePresenter(runtime, scope).also {
                            presenter = it
                            it.attach("hub")
                            it.openReceipt(StockFixtures.trace.id)
                        }
                    }
                val state by active.state.collectAsState()
                Column(Modifier.verticalScroll(rememberScrollState())) { TraceSourceCorrections(state, active) }
            }
            waitUntil { presenter?.state?.value?.snapshot != null }
            val first =
                original.sourceCorrections
                    .first()
                    .operationId.value
            val last =
                original.sourceCorrections
                    .last()
                    .operationId.value
            onNodeWithTag("trace-source-correction-$first").assertExists()
            onNodeWithTag("trace-source-correction-$last").assertDoesNotExist()
            onNodeWithText("Before: Unknown source; no relationship is established").assertExists()
            onNodeWithTag("trace-source-scope-$first").performScrollTo().performClick()
            onNodeWithTag("trace-source-affected-scope").assertExists()
            onNodeWithTag("trace-source-impact-previous").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("trace-source-impact-next").performScrollTo().assertIsEnabled().performClick()
            onNodeWithTag("trace-source-impact-next").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("trace-source-impact-previous").performScrollTo().performClick()
            onNodeWithTag("trace-source-next").performScrollTo().performClick()
            onNodeWithTag("trace-source-correction-$last").assertExists()
            onNodeWithTag("trace-source-correction-$first").assertDoesNotExist()
            onNodeWithTag("trace-source-next").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("trace-source-previous").performScrollTo().performClick()
            onNodeWithTag("trace-source-correction-$first").assertExists()
            runOnIdle { presenter?.close() }
        }
}
