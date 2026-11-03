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
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubTraceSnapshotQuery
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
class CustodyStockAcceptanceTest {
    @Test
    fun stockReturnDraftSurvivesNavigationButRequiresFreshContextAndConfirmation() = stockFlow(false)

    @Test
    fun coalescedSameOwnerNativeReplacementInvalidatesOldReturnFieldsAndRenderedCallback() = stockFlow(true)

    @Test
    fun stockInspectionDisposesOnlyTheExplicitlySelectedPhysicalPortion() = stockFlow(false, disposition = true)

    private fun stockFlow(
        replaceSession: Boolean,
        disposition: Boolean = false,
    ) = runDesktopComposeUiTest(width = 1100, height = 720) {
        val root = Files.createTempDirectory("harvestcircle-stock-custody-")
        val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
        var show by mutableStateOf(true)
        var closeRequested by mutableStateOf(false)
        var exits = 0
        try {
            val hub = runBlocking { HandlingFixture.open(runtime) }
            val target =
                if (disposition) {
                    runBlocking {
                        hub.split(hub.receipt.initialPosition.groupId, 2U, "Two explicitly selected orange crates")
                    }
                } else {
                    hub.receipt.initialPosition.groupId
                }
            val original = runBlocking { runtime.stock(HubStockQuery()) }
            setContent {
                HarvestCircleWindowChromeEnvironment(HarvestCircleWindowChromeExclusion.None) {
                    if (show) {
                        HarvestCircleApplicationWithDependencies(
                            closeRequested = closeRequested,
                            onExitApproved = { exits += 1 },
                            clipboardFactory = { scope -> SecretClipboardController(scope, CustodyClipboard()) },
                        ) { scope ->
                            HarvestCirclePresenter(
                                runtime,
                                scope,
                                ApplicationClock { UnixSeconds(1_700_000_000) },
                                OperationIdSource { OperationId.from(org.harvestcircle.ffi.generateOperationIdV7()) },
                            )
                        }
                    }
                }
            }
            waitUntil(timeoutMillis = 10_000) { onNodeWithTag("sidebar-HubStock").runCatching { fetchSemanticsNode() }.isSuccess }
            onNodeWithTag("sidebar-HubStock").performClick()
            if (disposition) {
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("stock-inspect-$target").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("stock-inspect-$target").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("custody-dispose").assertIsEnabled() }.isSuccess }
                onNodeWithTag("custody-dispose").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("custody-confirm").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("custody-quantity").assertDoesNotExist()
            } else {
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("custody-return").assertIsEnabled() }.isSuccess }
                onNodeWithTag("custody-return").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("custody-variant").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("custody-variant").performScrollTo().performClick()
                onNodeWithTag("custody-variant-${hub.receipt.trace.variant.id}").performScrollTo().performClick()
                onNodeWithTag("custody-location").performScrollTo().performClick()
                onNodeWithTag("custody-location-${hub.receiving}").performScrollTo().performClick()
                onNodeWithTag("custody-quantity").performScrollTo().performTextInput("2")
                onNodeWithTag("custody-label").performScrollTo().performTextInput("Two observed returned crates")
                onNodeWithTag("custody-identification").performScrollTo().performTextInput("Separate returned crates beside original stock")
            }
            onNodeWithTag(
                "custody-observation",
            ).performScrollTo().performTextInput(
                if (disposition) "Observed destruction of selected orange crates" else "Original departure and source remain unknown",
            )
            onNodeWithTag("custody-confirm").performScrollTo().performClick()
            assertEquals(original, runBlocking { runtime.stock(HubStockQuery()) })
            if (disposition) {
                onNodeWithTag("custody-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("custody-result").runCatching { fetchSemanticsNode() }.isSuccess }
                val text =
                    onNodeWithTag("custody-operation")
                        .fetchSemanticsNode()
                        .config[SemanticsProperties.Text]
                        .single()
                        .text
                val operation = OperationId.from(text.removePrefix("Original operation "))
                val result = runBlocking { assertNotNull(runtime.dispositionOperation(operation)) }
                assertEquals(target, result.before.id)
                assertEquals(2U, result.before.wholePacks)
                assertEquals(18UL, runBlocking { runtime.stock(HubStockQuery()).rows.sumOf { it.position.wholePacks.toULong() } })
                assertEquals(2U, runBlocking { runtime.traceSnapshot(HubTraceSnapshotQuery(hub.receipt.trace.id)).disposedWholePacks })
            } else if (replaceSession) {
                val oldAction = onNodeWithTag("custody-submit").fetchSemanticsNode().config[SemanticsActions.OnClick].action!!
                val before = runtime.currentSnapshot()
                runOnIdle {
                    runBlocking {
                        runtime.execute(ApplicationCommand.SignOut)
                        runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                    }
                    oldAction()
                }
                assertNotEquals(before.runtimeSession, runtime.currentSnapshot().runtimeSession)
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("custody-workspace").runCatching { fetchSemanticsNode() }.isFailure }
                onNodeWithTag("sidebar-HubStock").performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("custody-return").assertIsEnabled() }.isSuccess }
                onNodeWithTag("custody-return").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("custody-confirm").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("custody-submit").assertIsNotEnabled()
                assertEquals("", onNodeWithTag("custody-quantity").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
                assertEquals("", onNodeWithTag("custody-observation").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
                assertEquals(original, runBlocking { runtime.stock(HubStockQuery()) })
            } else {
                onNodeWithTag("sidebar-HubWork").performClick()
                onNodeWithTag("sidebar-HubStock").performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("custody-resume").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("custody-resume").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("custody-accept-context").assertIsEnabled() }.isSuccess }
                assertEquals("2", onNodeWithTag("custody-quantity").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
                onNodeWithTag("custody-submit").assertIsNotEnabled()
                onNodeWithTag("custody-accept-context").performScrollTo().performClick()
                onNodeWithTag("custody-submit").assertIsNotEnabled()
                onNodeWithTag("custody-confirm").performScrollTo().performClick()
                onNodeWithTag("custody-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("custody-result").runCatching { fetchSemanticsNode() }.isSuccess }
                assertEquals(22UL, runBlocking { runtime.stock(HubStockQuery()).rows.sumOf { it.position.wholePacks.toULong() } })
            }
            assertEquals(hub.receipt, runBlocking { runtime.receiptOperation(hub.receipt.operationId) })
            closeRequested = true
            waitUntil(timeoutMillis = 10_000) { exits == 1 }
            show = false
            waitForIdle()
        } finally {
            show = false
            runBlocking { runtime.close() }
            Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
        }
    }
}

private class CustodyClipboard : TextClipboard {
    private var value: String? = null

    override fun readText(): String? = value

    override fun writeText(value: String) {
        this.value = value.ifEmpty { null }
    }
}
