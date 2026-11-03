package org.harvestcircle.integration

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationClock
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.HarvestCircleApplicationWithDependencies
import org.harvestcircle.application.HarvestCirclePresenter
import org.harvestcircle.application.HubActualReportFilter
import org.harvestcircle.application.HubActualReportQuery
import org.harvestcircle.application.HubActualSourceKind
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.SecretClipboardController
import org.harvestcircle.application.TextClipboard
import org.harvestcircle.application.UnixSeconds
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeEnvironment
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeExclusion
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull

@OptIn(ExperimentalTestApi::class)
class ActualStockAcceptanceTest {
    @Test
    fun workStockNavigationKeepsActualReportDraftWithoutPostingStock() = stockFlow(false)

    @Test
    fun sameOwnerNativeRoundTripInvalidatesActualDraftAndRenderedSubmission() = stockFlow(true)

    private fun stockFlow(replaceSession: Boolean) =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-stock-actual-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            var show by mutableStateOf(true)
            var closeRequested by mutableStateOf(false)
            var exits = 0
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                val original = runBlocking { runtime.stock(HubStockQuery()) }
                setContent {
                    HarvestCircleWindowChromeEnvironment(HarvestCircleWindowChromeExclusion.None) {
                        if (show) {
                            HarvestCircleApplicationWithDependencies(
                                closeRequested = closeRequested,
                                onExitApproved = { exits++ },
                                clipboardFactory = { scope -> SecretClipboardController(scope, ActualClipboard()) },
                            ) { scope ->
                                HarvestCirclePresenter(
                                    runtime,
                                    scope,
                                    ApplicationClock {
                                        UnixSeconds(1_700_000_000)
                                    },
                                    OperationIdSource { OperationId.from(org.harvestcircle.ffi.generateOperationIdV7()) },
                                )
                            }
                        }
                    }
                }
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("sidebar-HubStock").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("sidebar-HubStock").performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("operations-actual-reports").assertIsEnabled() }.isSuccess }
                onNodeWithTag("operations-actual-reports").performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("actual-new-report").assertIsEnabled() }.isSuccess }
                onNodeWithTag("actual-new-report").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("actual-observation").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag(
                    "actual-observation",
                ).performScrollTo().performTextInput("Reported physical departure; source and recipient unverified")
                onNodeWithTag("actual-quantity").performScrollTo().performTextInput("2")
                onNodeWithTag("actual-confirm").performScrollTo().performClick()
                onNodeWithTag("actual-submit").assertIsEnabled()
                assertEquals(original, runBlocking { runtime.stock(HubStockQuery()) })
                if (replaceSession) {
                    val oldAction =
                        assertNotNull(onNodeWithTag("actual-submit").fetchSemanticsNode().config[SemanticsActions.OnClick].action)
                    val before = runtime.currentSnapshot()
                    runOnIdle {
                        runBlocking {
                            runtime.execute(ApplicationCommand.SignOut)
                            runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                        }
                        oldAction()
                    }
                    assertNotEquals(before.runtimeSession, runtime.currentSnapshot().runtimeSession)
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("actual-workspace").runCatching { fetchSemanticsNode() }.isFailure }
                    onNodeWithTag("sidebar-HubStock").performClick()
                    waitUntil(
                        timeoutMillis = 10_000,
                    ) { runCatching { onNodeWithTag("operations-actual-reports").assertIsEnabled() }.isSuccess }
                    onNodeWithTag("operations-actual-reports").performClick()
                    waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("actual-new-report").assertIsEnabled() }.isSuccess }
                    onNodeWithTag("actual-new-report").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("actual-observation").runCatching { fetchSemanticsNode() }.isSuccess }
                    onNodeWithTag("actual-submit").performScrollTo().assertIsNotEnabled()
                    assertEquals("", onNodeWithTag("actual-observation").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
                    assertEquals("", onNodeWithTag("actual-quantity").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
                    assertEquals(0U, runBlocking { runtime.actualReports(HubActualReportQuery(filter = HubActualReportFilter.All)) }.total)
                    assertEquals(original, runBlocking { runtime.stock(HubStockQuery()) })
                } else {
                    onNodeWithTag("sidebar-HubWork").performClick()
                    onNodeWithTag("sidebar-HubStock").performClick()
                    waitUntil(
                        timeoutMillis = 10_000,
                    ) { onNodeWithTag("operations-actual-reports").runCatching { fetchSemanticsNode() }.isSuccess }
                    onNodeWithTag("operations-actual-reports").performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("actual-submit").runCatching { fetchSemanticsNode() }.isSuccess }
                    assertEquals("2", onNodeWithTag("actual-quantity").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
                    onNodeWithTag("actual-submit").performScrollTo().assertIsNotEnabled()
                    onNodeWithTag("actual-confirm").performScrollTo().performClick()
                    onNodeWithTag("actual-submit").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("actual-done").runCatching { fetchSemanticsNode() }.isSuccess }
                    val text =
                        onNodeWithTag("actual-operation")
                            .fetchSemanticsNode()
                            .config[SemanticsProperties.Text]
                            .single()
                            .text
                    val operation = OperationId.from(text.removePrefix("Original operation "))
                    val report = runBlocking { assertNotNull(runtime.actualReportOperation(operation)) }
                    assertEquals(2U, report.reported.wholePacks)
                    assertEquals(HubActualSourceKind.Unknown, report.reported.sourceKind)
                    assertEquals(original.rows, runBlocking { runtime.stock(HubStockQuery()) }.rows)
                }
                assertEquals(hub.receipt, runBlocking { runtime.receiptOperation(hub.receipt.operationId) })
                closeRequested = true
                waitUntil(timeoutMillis = 10_000) { exits == 1 }
                show = false
                waitForIdle()
            } finally {
                show = false
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }
}

private class ActualClipboard : TextClipboard {
    private var value: String? = null

    override fun readText() = value

    override fun writeText(value: String) {
        this.value = value.ifEmpty { null }
    }
}
