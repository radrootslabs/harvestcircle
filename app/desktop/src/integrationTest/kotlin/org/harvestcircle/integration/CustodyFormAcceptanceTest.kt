package org.harvestcircle.integration

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import kotlinx.coroutines.runBlocking
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.CustodyPresenter
import org.harvestcircle.application.CustodyRequest
import org.harvestcircle.application.CustodyResult
import org.harvestcircle.application.CustodySubmission
import org.harvestcircle.application.HarvestCircleCustodyRuntime
import org.harvestcircle.application.HubAcceptanceStatus
import org.harvestcircle.application.HubHandlingInput
import org.harvestcircle.application.HubHandlingQuery
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubReturnInput
import org.harvestcircle.application.HubReturnReceipt
import org.harvestcircle.application.HubReturnResolution
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubTreatmentScope
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.RequestContext
import org.harvestcircle.ui.shell.CustodyWorkspace
import org.harvestcircle.ui.shell.HarvestCircleTheme
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class CustodyFormAcceptanceTest {
    @Test
    fun returnedTwoFormRequiresConfirmationAndRecoversLostReplyWithoutAddingStockAgain() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-return-form-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            lateinit var presenter: CustodyPresenter
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                val departed =
                    runBlocking {
                        val group = runtime.group(hub.receipt.initialPosition.groupId)
                        runtime.applyHandling(
                            hub.context(),
                            HubHandlingInput.SetAcceptance(
                                group.stockRevision,
                                group.catalogRevision,
                                listOf(group.group.id),
                                HubAcceptanceStatus.Accepted,
                                HubOccurrence(HubOccurrenceKind.Unknown),
                                "Original twenty accepted",
                            ),
                        )
                        val six = hub.split(group.group.id, 6U, "Six blue-tagged original crates")
                        val paper = custodyIssue(hub, listOf(six), "Original recipient")
                        runtime.applyDeparture(hub.context(), custodyDepartureInput(runtime, paper))
                    }
                val lostReply =
                    object : HarvestCircleCustodyRuntime by runtime {
                        override suspend fun applyReturn(
                            context: RequestContext,
                            input: HubReturnInput,
                        ): HubReturnReceipt {
                            runtime.applyReturn(context, input)
                            error("Test-owned reply loss after the native commit")
                        }
                    }
                val hubId = runBlocking { assertNotNull(runtime.hubContext()).id }
                setContent { NativeCustodyForm(runtime, lostReply, hubId, { it.beginReturn() }) { presenter = it } }
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null }
                onNodeWithTag("custody-variant").performScrollTo().performClick()
                onNodeWithTag("custody-variant-${hub.receipt.trace.variant.id}").performScrollTo().performClick()
                onNodeWithTag("custody-location").performScrollTo().performClick()
                onNodeWithTag("custody-location-${hub.receiving}").performScrollTo().performClick()
                onNodeWithTag("custody-quantity").performScrollTo().performTextInput("2")
                onNodeWithTag("custody-label").performScrollTo().performTextInput("Two yellow-tagged returned crates")
                onNodeWithTag(
                    "custody-identification",
                ).performScrollTo().performTextInput("Actual returned crates separate from original fourteen")
                onNodeWithTag("custody-departure-operation").performScrollTo().performTextInput(departed.operationId.value)
                onNodeWithTag("custody-read-departure").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    presenter.state.value.draft
                        ?.departure != null &&
                        !presenter.state.value.loading
                }
                assertTrue(requireNotNull(presenter.state.value.draft).fields.originalGroupId.isEmpty())
                onNodeWithText("Six blue-tagged original crates").performScrollTo().assertExists()
                onNodeWithTag("custody-associate-${departed.lines.single().groupId}").performScrollTo().performClick()
                onNodeWithTag(
                    "custody-observation",
                ).performScrollTo().performTextInput("Observed return; source and actual time remain unknown")
                onNodeWithTag("custody-submit").assertIsNotEnabled()
                assertEquals(14UL, runBlocking { runtime.stock(HubStockQuery()).rows.sumOf { it.position.wholePacks.toULong() } })
                onNodeWithTag("custody-confirm").performScrollTo().performClick()
                onNodeWithTag("custody-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CustodySubmission.Uncertain }
                val attempt = assertIs<CustodySubmission.Uncertain>(presenter.state.value.submission).attempt
                assertEquals(16UL, runBlocking { runtime.stock(HubStockQuery()).rows.sumOf { it.position.wholePacks.toULong() } })
                onNodeWithTag("custody-discard").assertDoesNotExist()
                onNodeWithTag("custody-submit").assertDoesNotExist()
                onNodeWithTag("custody-check-original").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CustodySubmission.Recorded }
                val result =
                    assertIs<CustodyResult.Returned>(
                        assertIs<CustodySubmission.Recorded>(presenter.state.value.submission).result,
                    ).receipt
                assertEquals(attempt.operation, result.operationId)
                assertEquals(HubReturnResolution.EstablishedScope, result.resolution)
                assertEquals(HubAcceptanceStatus.ReviewRequired, result.acceptance)
                onNodeWithText("Occurrence: Unknown").assertExists()
                onNodeWithText("Count time: Unknown").assertDoesNotExist()
                val old = runBlocking { runtime.treatment(HubTreatmentScope(hub.receipt.trace.id)) }
                val returned = runBlocking { runtime.treatment(HubTreatmentScope(result.population.id)) }
                assertEquals(14U, old.summary.departureEligible)
                assertEquals(2U, returned.summary.reviewRequired)
                assertEquals(0U, returned.summary.departureEligible)
                val replay =
                    runBlocking {
                        runtime.applyReturn(
                            hub.context().copy(operationId = attempt.operation),
                            assertIs<CustodyRequest.Return>(attempt.request).input,
                        )
                    }
                assertEquals(result, replay)
                assertEquals(16UL, runBlocking { runtime.stock(HubStockQuery()).rows.sumOf { it.position.wholePacks.toULong() } })
                assertEquals(departed, runBlocking { runtime.departureOperation(departed.operationId) })
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }

    @Test
    fun dispositionFormRemovesOnlyActuallySeparatedTwoRetainsCompletedWorkAndHoldAndReadsOriginalAfterRestart() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-disposition-form-")
            var incarnation by mutableStateOf(0)
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            lateinit var presenter: CustodyPresenter
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                val two =
                    runBlocking {
                        val group = runtime.group(hub.receipt.initialPosition.groupId)
                        hub.hold(listOf(group.group.id), "Precautionary hold must remain retained")
                        runtime.applyHandling(hub.context(), hub.move(runtime.group(group.group.id), hub.a3, 8U))
                        hub.split(group.group.id, 2U, "Two orange crates actually separated for destruction")
                    }
                val before = runBlocking { runtime.stock(HubStockQuery()) }
                val hubId = runBlocking { assertNotNull(runtime.hubContext()).id }
                setContent {
                    key(incarnation) {
                        NativeCustodyForm(
                            runtime,
                            runtime,
                            hubId,
                            { if (incarnation == 0) it.beginDisposition(two) else it.beginOriginalLookup() },
                        ) { presenter = it }
                    }
                }
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null }
                onNodeWithTag(
                    "custody-observation",
                ).performScrollTo().performTextInput("Actual destruction of the two physically separated orange crates")
                onNodeWithTag("custody-submit").assertIsNotEnabled()
                assertEquals(before, runBlocking { runtime.stock(HubStockQuery()) })
                onNodeWithTag("custody-confirm").performScrollTo().performClick()
                onNodeWithTag("custody-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CustodySubmission.Recorded }
                val recorded = assertIs<CustodySubmission.Recorded>(presenter.state.value.submission)
                val result = assertIs<CustodyResult.Disposed>(recorded.result).receipt
                assertEquals(two, result.before.id)
                assertEquals(2U, result.before.wholePacks)
                assertEquals(18UL, runBlocking { runtime.stock(HubStockQuery()).rows.sumOf { it.position.wholePacks.toULong() } })
                val work =
                    runBlocking {
                        runtime
                            .work(HubHandlingQuery(openOnly = false))
                            .rows
                            .single()
                            .work.quantity
                    }
                assertEquals(listOf(20U, 8U, 2U, 10U), listOf(work.required, work.completed, work.noLongerRequired, work.remaining))
                val treatment = runBlocking { runtime.treatment(HubTreatmentScope(hub.receipt.trace.id)) }
                assertTrue(treatment.holds.isNotEmpty())
                assertEquals(18U, treatment.summary.restricted)
                assertNull(
                    treatment.holds
                        .single()
                        .hold.release,
                )
                onNodeWithText("Actual occurrence: Unknown").assertExists()
                val replay =
                    runBlocking {
                        runtime.applyDisposition(
                            hub.context().copy(operationId = recorded.attempt.operation),
                            assertIs<CustodyRequest.Dispose>(recorded.attempt.request).input,
                        )
                    }
                assertEquals(result, replay)
                val after = runBlocking { runtime.stock(HubStockQuery()) }
                onNodeWithTag("custody-done").performScrollTo().performClick()
                runBlocking {
                    runtime.restart()
                    runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                }
                val oldPresenter = presenter
                runOnIdle { incarnation += 1 }
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.originalLookup != null }
                onNodeWithTag("custody-lookup-kind").performScrollTo().performClick()
                onNodeWithText("Disposition").performClick()
                onNodeWithTag("custody-lookup-operation").performScrollTo().performTextInput(result.operationId.value)
                onNodeWithTag("custody-lookup-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    presenter.state.value.originalLookup
                        ?.result != null
                }
                assertEquals(
                    result,
                    assertIs<CustodyResult.Disposed>(
                        presenter.state.value.originalLookup
                            ?.result,
                    ).receipt,
                )
                runOnIdle {
                    oldPresenter.beginReturn()
                    oldPresenter.confirm(true)
                    oldPresenter.submit(runtime.currentSnapshot().revision)
                }
                assertFalse(oldPresenter.state.value.available)
                onNodeWithText("Actual occurrence: Unknown").assertExists()
                assertEquals(after, runBlocking { runtime.stock(HubStockQuery()) })
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }
}

@Composable
private fun NativeCustodyForm(
    runtime: TestBridgeHarvestCircleRuntime,
    custody: HarvestCircleCustodyRuntime,
    hub: String,
    begin: (CustodyPresenter) -> Unit,
    ready: (CustodyPresenter) -> Unit,
) {
    val snapshot = runtime.currentSnapshot()
    val scope = rememberCoroutineScope()
    val presenter =
        remember(runtime, snapshot.runtimeSession, hub) {
            CustodyPresenter(
                custody,
                runtime,
                runtime,
                runtime,
                runtime,
                scope,
                OperationIdSource {
                    OperationId.from(org.harvestcircle.ffi.generateOperationIdV7())
                },
            )
        }
    DisposableEffect(presenter) { onDispose { presenter.close() } }
    LaunchedEffect(presenter) {
        presenter.attach(hub, requireNotNull(snapshot.runtimeSession))
        begin(presenter)
    }
    ready(presenter)
    val state by presenter.state.collectAsState()
    HarvestCircleTheme(AppearanceState()) { CustodyWorkspace(state, presenter) { presenter.submit(snapshot.revision) } }
}
