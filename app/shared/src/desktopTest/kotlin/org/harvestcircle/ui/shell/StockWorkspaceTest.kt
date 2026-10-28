package org.harvestcircle.ui.shell

import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.HarvestCircleReferenceRuntime
import org.harvestcircle.application.HarvestCircleStockRuntime
import org.harvestcircle.application.HubReceiptInput
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferencePage
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceReceipt
import org.harvestcircle.application.HubSourceCertainty
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubStockReceipt
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.ReceiptAttempt
import org.harvestcircle.application.ReceiptSubmission
import org.harvestcircle.application.RequestContext
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.application.StockFixtures
import org.harvestcircle.application.StockPresenter
import org.harvestcircle.application.StockViewState
import org.harvestcircle.navigation.AppRoute
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class StockWorkspaceTest {
    @Test
    fun receiptFormSendsActualReceivingAndPlannedLocationAsDistinctNativeFields() =
        runComposeUiTest {
            val runtime = StockUiRuntime()
            lateinit var presenter: StockPresenter
            setHarvestCircleContent {
                val scope = rememberCoroutineScope()
                presenter = remember { StockPresenter(runtime, runtime, scope, OperationIdSource { StockFixtures.operation }) }
                val state by presenter.state.collectAsState()
                LaunchedEffect(Unit) { presenter.attach("hub") }
                StockWorkspace(AppRoute.HubStock, state, presenter, { presenter.submitReceipt(SnapshotRevision(7UL)) }, {})
            }
            waitUntil { presenter.state.value.page != null }
            onNodeWithTag("stock-receive").performClick()
            onNodeWithTag("receipt-variant").performScrollTo().performClick()
            onNodeWithTag("receipt-variant-${StockFixtures.pack.id}").performScrollTo().performClick()
            onNodeWithTag("receipt-quantity").performScrollTo().performTextInput("20")
            onNodeWithTag("receipt-receiving").performScrollTo().performClick()
            onNodeWithTag("receipt-receiving-${StockFixtures.receiving.id}").performScrollTo().performClick()
            onNodeWithTag("receipt-planned").performScrollTo().performClick()
            onNodeWithTag("receipt-planned-${StockFixtures.planned.id}").performScrollTo().performClick()
            onNodeWithTag("receipt-observation").performScrollTo().performTextInput("Counted twenty original crates")
            onNodeWithTag("receipt-save").performScrollTo().performClick()
            waitUntil { presenter.state.value.submission is ReceiptSubmission.Recorded }
            onNodeWithTag("receipt-recorded").performScrollTo().assertIsDisplayed()
            val input = runtime.inputs.single()
            assertEquals(StockFixtures.receiving.id, input.receivingId)
            assertEquals(StockFixtures.planned.id, input.plannedLocationId)
            assertEquals(20U, input.wholePacks)
            assertEquals(HubSourceCertainty.Unknown, input.source.certainty)
            onNodeWithTag("receipt-done").performScrollTo().performClick()
            waitUntil {
                presenter.state.value.submission == null &&
                    presenter.state.value.page
                        ?.total == 1U
            }
            onNodeWithText("At: Receiving").performScrollTo().assertIsDisplayed()
            runOnIdle { presenter.close() }
        }

    @Test
    fun uncertainOutcomeHasOriginalLookupAndCannotStartAnotherReceipt() =
        runComposeUiTest {
            val input =
                HubReceiptInput(
                    1UL,
                    5UL,
                    StockFixtures.pack.id,
                    null,
                    null,
                    org.harvestcircle.application.HubSourceInput(),
                    20U,
                    StockFixtures.receiving.id,
                    StockFixtures.planned.id,
                    org.harvestcircle.application.HubOccurrence(org.harvestcircle.application.HubOccurrenceKind.Unknown),
                    "Counted",
                )
            val state =
                StockViewState(
                    available = true,
                    submission = ReceiptSubmission.Uncertain(ReceiptAttempt(StockFixtures.operation, input), "Unconfirmed"),
                )
            setHarvestCircleContent {
                StockWorkspace(
                    AppRoute.HubStock,
                    state,
                    StockPresenter(
                        null,
                        null,
                        rememberCoroutineScope(),
                        OperationIdSource {
                            StockFixtures.operation
                        },
                    ),
                    {},
                    {},
                )
            }
            onNodeWithTag("receipt-check-original").performScrollTo().assertIsEnabled()
            onNodeWithTag("receipt-save").assertDoesNotExist()
            onNodeWithTag("stock-receive").assertDoesNotExist()
        }
}

private class StockUiRuntime :
    HarvestCircleStockRuntime,
    HarvestCircleReferenceRuntime {
    val inputs = mutableListOf<HubReceiptInput>()

    override suspend fun stock(query: HubStockQuery) = if (inputs.isEmpty()) StockFixtures.empty else StockFixtures.received

    override suspend fun receiveStock(
        context: RequestContext,
        input: HubReceiptInput,
    ): HubStockReceipt {
        inputs += input
        return StockFixtures.receipt(context.operationId)
    }

    override suspend fun receiptOperation(operation: OperationId): HubStockReceipt? =
        if (inputs.isEmpty()) null else StockFixtures.receipt(operation)

    override suspend fun references(query: HubReferenceQuery) =
        HubReferencePage(StockFixtures.basis, 5UL, 0U, 4U, false, StockFixtures.references)

    override suspend fun editReference(
        context: RequestContext,
        edit: HubReferenceEdit,
    ): HubReferenceReceipt = error("Unexpected edit")

    override suspend fun referenceOperation(operation: OperationId): HubReferenceReceipt? = error("Unexpected lookup")

    override suspend fun referenceHistory(
        id: String,
        afterRevision: ULong,
        limit: UInt,
    ): List<HubReferenceReceipt> = error("Unexpected history")
}
