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
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.StockFixtures
import org.harvestcircle.application.TraceFake
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.application.traceRecipientRows
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class TraceRecipientCorrectionsTest {
    @Test
    fun recipientSearchResultRetainsAmendmentCountAndOriginalEventScopeBeforeOpening() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val corrections = traceRecipientRows(1)
            val base = TraceFake().page
            val result = base.copy(stockRevision = 3UL, rows = listOf(base.rows.single().copy(recipientCorrections = corrections)))
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
            onNodeWithText("1 linked recipient amendments; original event and paper retained").assertExists()
            onNodeWithText("Open the recorded trace to inspect each event's original and current recipient interpretation.").assertExists()
            runOnIdle { presenter?.close() }
        }

    @Test
    fun renderedRecipientHistoryAndAffectedReferencesKeepOriginalAndUnknownMeaningsPaged() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val original = TraceFake().snapshot.copy(recipientCorrections = traceRecipientRows(65))
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
                Column(Modifier.verticalScroll(rememberScrollState())) { TraceRecipientCorrections(state, active) }
            }
            waitUntil { presenter?.state?.value?.snapshot != null }
            val first =
                original.recipientCorrections
                    .first()
                    .operationId.value
            val last =
                original.recipientCorrections
                    .last()
                    .operationId.value
            onNodeWithTag("trace-recipient-correction-$first").assertExists()
            onNodeWithTag("trace-recipient-correction-$last").assertDoesNotExist()
            onNodeWithText("After: Unknown recipient; no party is established").assertExists()
            onNodeWithTag("trace-recipient-scope-$first").performScrollTo().performClick()
            onNodeWithTag("trace-recipient-affected-scope").assertExists()
            onNodeWithTag("trace-recipient-impact-previous").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("trace-recipient-impact-next").performScrollTo().assertIsEnabled().performClick()
            onNodeWithTag("trace-recipient-impact-next").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("trace-recipient-impact-previous").performScrollTo().performClick()
            onNodeWithTag("trace-recipient-next").performScrollTo().performClick()
            onNodeWithTag("trace-recipient-correction-$last").assertExists()
            onNodeWithTag("trace-recipient-correction-$first").assertDoesNotExist()
            onNodeWithTag("trace-recipient-next").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("trace-recipient-previous").performScrollTo().performClick()
            onNodeWithTag("trace-recipient-correction-$first").assertExists()
            runOnIdle { presenter?.close() }
        }

    @Test
    fun eachEventShowsItsOwnCurrentRecipientInsteadOfTheLastUnrelatedAmendment() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val corrections =
                traceRecipientRows(2).mapIndexed { index, row ->
                    if (index == 0) {
                        row
                    } else {
                        row.copy(
                            originalDepartureOperationId = OperationId.from("01900000-0000-7000-8000-000000000901"),
                            previousOperationId = null,
                            before = row.originalRecipient,
                        )
                    }
                }
            val original = TraceFake().snapshot.copy(recipientCorrections = corrections)
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
                Column(Modifier.verticalScroll(rememberScrollState())) { TraceRecipientCorrections(state, active) }
            }
            waitUntil { presenter?.state?.value?.snapshot != null }
            onNodeWithText("Current recipient interpretation for this event: Unknown recipient; no party is established").assertExists()
            onNodeWithText("Current recipient interpretation for this event: ${traceRecipientMeaning(corrections[1].after)}").assertExists()
            runOnIdle { presenter?.close() }
        }
}
