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
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationClock
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.HarvestCircleApplicationWithDependencies
import org.harvestcircle.application.HarvestCirclePresenter
import org.harvestcircle.application.HubHandlingQuery
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

@OptIn(ExperimentalTestApi::class)
class CountStockAcceptanceTest {
    @Test
    fun stockFormSavesThenPostsAndRetainsOriginalReceipt() = stockScenario(replaceSession = false)

    @Test
    fun coalescedSameOwnerReplacementDropsConfirmationAndOldRenderedAction() = stockScenario(replaceSession = true)

    private fun stockScenario(replaceSession: Boolean) =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-stock-count-")
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
                                onExitApproved = { exits += 1 },
                                clipboardFactory = { scope -> SecretClipboardController(scope, CountClipboard()) },
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
                val groupId = hub.receipt.initialPosition.groupId
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("stock-inspect-$groupId").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("stock-inspect-$groupId").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("count-observe").assertIsEnabled() }.isSuccess }
                onNodeWithTag("count-observe").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("count-quantity").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("count-submit").assertIsNotEnabled()
                onNodeWithTag("count-quantity").performScrollTo().performTextInput("18")
                onNodeWithTag("count-cutoff").performScrollTo().performTextReplacement("2023-11-14T22:13:20Z")
                onNodeWithTag("count-observation").performScrollTo().performTextInput("Eighteen original packs physically counted")
                onNodeWithTag("count-confirm").performScrollTo().performClick()
                if (replaceSession) {
                    val oldAction = onNodeWithTag("count-submit").fetchSemanticsNode().config[SemanticsActions.OnClick].action!!
                    val before = runtime.currentSnapshot()
                    runOnIdle {
                        runBlocking {
                            runtime.execute(ApplicationCommand.SignOut)
                            runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                        }
                        oldAction()
                    }
                    assertNotEquals(before.runtimeSession, runtime.currentSnapshot().runtimeSession)
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("count-workspace").runCatching { fetchSemanticsNode() }.isFailure }
                    onNodeWithTag("sidebar-HubStock").performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("stock-workspace").runCatching { fetchSemanticsNode() }.isSuccess }
                    assertEquals(original, runBlocking { runtime.stock(HubStockQuery()) })
                    onNodeWithTag("stock-inspect-$groupId").performScrollTo().performClick()
                    onNodeWithTag("count-observe").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("count-confirm").runCatching { fetchSemanticsNode() }.isSuccess }
                    onNodeWithTag("count-submit").assertIsNotEnabled()
                    assertEquals("", onNodeWithTag("count-quantity").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
                    assertEquals("", onNodeWithTag("count-observation").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
                } else {
                    onNodeWithTag("sidebar-HubWork").performClick()
                    onNodeWithTag("sidebar-HubStock").performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("count-resume").runCatching { fetchSemanticsNode() }.isSuccess }
                    onNodeWithTag("count-resume").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("count-accept-review").assertIsEnabled() }.isSuccess }
                    assertEquals("18", onNodeWithTag("count-quantity").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
                    assertEquals(
                        "Eighteen original packs physically counted",
                        onNodeWithTag("count-observation").fetchSemanticsNode().config[SemanticsProperties.EditableText].text,
                    )
                    onNodeWithTag("count-submit").assertIsNotEnabled()
                    onNodeWithTag("count-accept-review").performScrollTo().performClick()
                    onNodeWithTag("count-submit").assertIsNotEnabled()
                    onNodeWithTag("count-confirm").performScrollTo().performClick()
                    onNodeWithTag("count-submit").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("count-recorded").runCatching { fetchSemanticsNode() }.isSuccess }
                    val saved = runBlocking { runtime.stock(HubStockQuery()) }
                    assertEquals(original.rows, saved.rows)
                    assertEquals(original.stockRevision + 1UL, saved.stockRevision)
                    onNodeWithTag("count-review-post").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("count-confirm").runCatching { fetchSemanticsNode() }.isSuccess }
                    onNodeWithTag("count-submit").assertIsNotEnabled()
                    onNodeWithTag(
                        "count-observation",
                    ).performScrollTo().performTextInput("Reviewed original count and current consequences")
                    onNodeWithTag("count-confirm").performScrollTo().performClick()
                    onNodeWithTag("count-submit").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("count-recorded").runCatching { fetchSemanticsNode() }.isSuccess }
                    assertEquals(
                        18U,
                        runBlocking { runtime.stock(HubStockQuery()) }
                            .rows
                            .single()
                            .position.wholePacks,
                    )
                    val work =
                        runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }
                            .rows
                            .single()
                            .work.quantity
                    assertEquals(listOf(0U, 2U, 18U), listOf(work.completed, work.noLongerRequired, work.remaining))
                }
                assertEquals(hub.receipt, runBlocking { runtime.receiptOperation(hub.receipt.operationId) })
                closeRequested = true
                waitUntil(timeoutMillis = 10_000) { exits == 1 }
                show = false
                waitForIdle()
            } finally {
                show = false
                runBlocking { runtime.close() }
                root.toFile().deleteRecursively()
            }
        }
}

private class CountClipboard : TextClipboard {
    private var value: String? = null

    override fun readText(): String? = value

    override fun writeText(value: String) {
        this.value = value.ifEmpty { null }
    }
}
