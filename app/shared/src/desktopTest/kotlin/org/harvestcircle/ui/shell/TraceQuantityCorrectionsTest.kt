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
import org.harvestcircle.application.HubTraceSnapshotQuery
import org.harvestcircle.application.StockFixtures
import org.harvestcircle.application.TraceFake
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.application.traceCorrectionRows
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class TraceQuantityCorrectionsTest {
    @Test
    fun renderedCorrectionPagesKeepOriginalAndCurrentMeaningWithoutAWriteAction() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val original = TraceFake().snapshot.copy(quantityCorrections = traceCorrectionRows(65))
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
                Column(Modifier.verticalScroll(rememberScrollState())) { TraceQuantityCorrections(state, active) }
            }
            waitUntil { presenter?.state?.value?.snapshot != null }
            val amendments = original.quantityCorrections.single().amendments
            onNodeWithTag("trace-correction-${amendments.first().operationId.value}").assertExists()
            onNodeWithTag("trace-correction-${amendments.last().operationId.value}").assertDoesNotExist()
            onNodeWithTag("trace-corrections-previous").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("trace-corrections-next").performScrollTo().assertIsEnabled().performClick()
            onNodeWithTag("trace-correction-${amendments.last().operationId.value}").assertExists()
            onNodeWithTag("trace-correction-${amendments.first().operationId.value}").assertDoesNotExist()
            onNodeWithText("Original 100 · Current 35 · Established returns 2").assertExists()
            onNodeWithText("Independently identified found remainder 1 · Before 36 · After 35").assertExists()
            onNodeWithTag("trace-corrections-next").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("trace-corrections-previous").performScrollTo().assertIsEnabled().performClick()
            onNodeWithTag("trace-correction-${amendments.first().operationId.value}").assertExists()
            runOnIdle { presenter?.close() }
        }
}
