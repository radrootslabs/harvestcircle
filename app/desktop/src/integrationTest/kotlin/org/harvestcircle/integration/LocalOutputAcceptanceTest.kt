package org.harvestcircle.integration

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationClock
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.DesktopLocalOutputPort
import org.harvestcircle.application.HarvestCircleApplicationWithDependencies
import org.harvestcircle.application.HarvestCirclePresenter
import org.harvestcircle.application.HubDispatchActionInput
import org.harvestcircle.application.HubDispatchEditInput
import org.harvestcircle.application.HubDispatchInput
import org.harvestcircle.application.HubDispatchLineInput
import org.harvestcircle.application.HubDispatchMaterialInput
import org.harvestcircle.application.HubDispatchReceipt
import org.harvestcircle.application.HubLocalOutput
import org.harvestcircle.application.HubLocalOutputRequest
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubTraceSnapshotQuery
import org.harvestcircle.application.LocalOutputAction
import org.harvestcircle.application.LocalOutputPort
import org.harvestcircle.application.LocalOutputResult
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.SecretClipboardController
import org.harvestcircle.application.TextClipboard
import org.harvestcircle.application.UnixSeconds
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeEnvironment
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeExclusion
import java.nio.file.Files
import java.nio.file.Path
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class LocalOutputAcceptanceTest {
    @Test
    fun nativeTraceOutputCancellationAtomicSavingAndPrintRetryNeverChangeStock() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-output-ui-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.resolve("runtime").toString())
            val port = ControlledOutputPort()
            var show by mutableStateOf(true)
            var closeRequested by mutableStateOf(false)
            var exits = 0
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                val before = runBlocking { runtime.stock(HubStockQuery()) }
                val expected =
                    runBlocking {
                        val snapshot = runtime.traceSnapshot(HubTraceSnapshotQuery(hub.receipt.trace.id))
                        runtime.localOutput(
                            HubLocalOutputRequest.Trace(snapshot.trace.id, snapshot.stockRevision, snapshot.catalogRevision),
                        )
                    }
                setContent {
                    if (show) OutputTestApplication(runtime, port, closeRequested) { exits += 1 }
                }
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("trace-browse").assertIsEnabled() }.isSuccess }
                onNodeWithTag("trace-browse").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    onNodeWithTag("trace-open-${hub.receipt.trace.id}").runCatching { fetchSemanticsNode() }.isSuccess
                }
                onNodeWithTag("trace-open-${hub.receipt.trace.id}").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("trace-output").assertIsEnabled() }.isSuccess }
                onNodeWithTag("trace-output").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("local-output-save").assertIsEnabled() }.isSuccess }
                assertEquals(
                    expected.identity,
                    onNodeWithTag(
                        "local-output-identity",
                    ).fetchSemanticsNode().config[androidx.compose.ui.semantics.SemanticsProperties.Text].single().text,
                )
                onNodeWithTag("local-output-save").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    onNodeWithText("Cancelled before output submission.", substring = true).runCatching { fetchSemanticsNode() }.isSuccess
                }
                val destination = root.resolve("captured-trace.html")
                port.destination = destination
                onNodeWithTag("local-output-save").performScrollTo().performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { onNodeWithText("Saved locally:", substring = true).runCatching { fetchSemanticsNode() }.isSuccess }
                assertEquals(expected.html, Files.readString(destination))
                val directory = Files.createDirectory(root.resolve("existing-directory"))
                Files.writeString(directory.resolve("retain.txt"), "retain")
                port.destination = directory
                onNodeWithTag("local-output-save").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    onNodeWithText("Output may have reached", substring = true).runCatching { fetchSemanticsNode() }.isSuccess
                }
                assertEquals("retain", Files.readString(directory.resolve("retain.txt")))
                assertEquals(expected.html, Files.readString(destination))
                port.printResult = LocalOutputResult.Uncertain
                onNodeWithTag("local-output-print").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("local-output-review").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("local-output-print").assertIsNotEnabled()
                port.destination = destination
                onNodeWithTag("local-output-save").performScrollTo().performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { onNodeWithText("Saved locally:", substring = true).runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("local-output-print").assertIsNotEnabled()
                onNodeWithTag("local-output-review").performScrollTo().performClick()
                port.printResult = LocalOutputResult.PrintSubmitted
                onNodeWithTag("local-output-print").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    onNodeWithText("Submitted to the print system.", substring = true).runCatching { fetchSemanticsNode() }.isSuccess
                }
                assertTrue(port.documents.isNotEmpty())
                assertTrue(port.documents.all { it == expected })
                assertEquals(before, runBlocking { runtime.stock(HubStockQuery()) })
                onNodeWithTag("local-output-back").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("trace-balance").runCatching { fetchSemanticsNode() }.isSuccess }
                closeRequested = true
                waitUntil(timeoutMillis = 10_000) { exits == 1 }
                show = false
                waitForIdle()
                runBlocking {
                    runtime.restart()
                    runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                    assertEquals(before, runtime.stock(HubStockQuery()))
                    assertEquals(
                        expected,
                        runtime.localOutput(
                            HubLocalOutputRequest.Trace(hub.receipt.trace.id, before.stockRevision, before.catalogRevision),
                        ),
                    )
                }
                assertFalse(
                    Files.list(root).use { files ->
                        files.anyMatch { it.fileName.toString().startsWith(".harvestcircle-output-") }
                    },
                )
            } finally {
                show = false
                port.close()
                runtime.close()
                root.toFile().deleteRecursively()
            }
        }

    @Test
    fun selectedSupersededIssuedVersionSavesItsOriginalContentAfterRenameAndRestart() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-issued-output-ui-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.resolve("runtime").toString())
            val port = ControlledOutputPort()
            var show by mutableStateOf(true)
            var closeRequested by mutableStateOf(false)
            var exits = 0
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                val issued = runBlocking { issueForOutput(hub) }
                val original = issued.record.versions.single()
                val request = HubLocalOutputRequest.Identification(issued.record.id, original.id)
                val expected = runBlocking { runtime.localOutput(request) }
                runBlocking {
                    val catalog = runtime.references(HubReferenceQuery())
                    val recipient = original.material.recipient
                    runtime.editReference(
                        hub.context(),
                        HubReferenceEdit(
                            catalog.catalogRevision,
                            recipient.id,
                            recipient.revision,
                            HubReferenceValue(HubReferenceKind.Party, "Current renamed recipient"),
                        ),
                    )
                    val current = runtime.group(hub.receipt.initialPosition.groupId)
                    runtime.applyDispatch(
                        hub.context(),
                        HubDispatchInput.Replace(
                            HubDispatchEditInput(
                                issued.record.id,
                                issued.record.revision,
                                HubDispatchMaterialInput(
                                    current.stockRevision,
                                    current.catalogRevision,
                                    recipient.id,
                                    listOf(HubDispatchLineInput(current.group.id, current.group.revision, 20U)),
                                    "A later replacement",
                                ),
                            ),
                        ),
                    )
                    runtime.restart()
                    runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                    assertEquals(expected, runtime.localOutput(request))
                }
                val before = runBlocking { runtime.stock(HubStockQuery()) }
                val paper = runBlocking { runtime.dispatchRecord(issued.record.id) }
                val destination = root.resolve("original-issued.html")
                port.destination = destination
                setContent { if (show) OutputTestApplication(runtime, port, closeRequested) { exits += 1 } }
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("sidebar-HubDispatch").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("sidebar-HubDispatch").performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { runCatching { onNodeWithTag("dispatch-record-${issued.record.id}").assertIsEnabled() }.isSuccess }
                onNodeWithTag("dispatch-record-${issued.record.id}").performScrollTo().performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { runCatching { onNodeWithTag("dispatch-version-${original.id}").assertIsEnabled() }.isSuccess }
                onNodeWithTag("dispatch-version-${original.id}").performScrollTo().performClick()
                onNodeWithTag("dispatch-output").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("local-output-save").assertIsEnabled() }.isSuccess }
                onNodeWithTag("local-output-save").performScrollTo().performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { onNodeWithText("Saved locally:", substring = true).runCatching { fetchSemanticsNode() }.isSuccess }
                assertEquals(expected.html, Files.readString(destination))
                assertEquals(listOf(expected), port.documents.toList())
                assertEquals(before, runBlocking { runtime.stock(HubStockQuery()) })
                assertEquals(paper, runBlocking { runtime.dispatchRecord(issued.record.id) })
                closeRequested = true
                waitUntil(timeoutMillis = 10_000) { exits == 1 }
                show = false
                waitForIdle()
            } finally {
                show = false
                port.close()
                runtime.close()
                root.toFile().deleteRecursively()
            }
        }
}

@Composable
private fun OutputTestApplication(
    runtime: TestBridgeHarvestCircleRuntime,
    port: LocalOutputPort,
    closeRequested: Boolean,
    onExit: () -> Unit,
) {
    HarvestCircleWindowChromeEnvironment(HarvestCircleWindowChromeExclusion.None) {
        HarvestCircleApplicationWithDependencies(
            closeRequested = closeRequested,
            onExitApproved = onExit,
            clipboardFactory = { scope -> SecretClipboardController(scope, OutputClipboard()) },
            outputPortFactory = { port },
        ) { scope ->
            val ids = OperationIdSource { OperationId.from(org.harvestcircle.ffi.generateOperationIdV7()) }
            HarvestCirclePresenter(runtime, scope, ApplicationClock { UnixSeconds(1_700_000_000) }, ids)
        }
    }
}

private suspend fun issueForOutput(hub: HandlingFixture): HubDispatchReceipt {
    val runtime = hub.runtime
    val catalog = runtime.references(HubReferenceQuery())
    val recipient =
        runtime
            .editReference(
                hub.context(),
                HubReferenceEdit(
                    catalog.catalogRevision,
                    value = HubReferenceValue(HubReferenceKind.Party, "Original recipient <sample>"),
                ),
            ).current
    val group = runtime.group(hub.receipt.initialPosition.groupId)
    val prepared =
        runtime.applyDispatch(
            hub.context(),
            HubDispatchInput.Prepare(
                HubDispatchMaterialInput(
                    group.stockRevision,
                    group.catalogRevision,
                    recipient.id,
                    listOf(HubDispatchLineInput(group.group.id, group.group.revision, 20U)),
                    "Original identification <script>never active</script>",
                ),
            ),
        )
    val current = runtime.dispatchRecord(prepared.record.id)
    return runtime.applyDispatch(
        hub.context(),
        HubDispatchInput.Issue(
            HubDispatchActionInput(
                current.stockRevision,
                current.catalogRevision,
                current.record.id,
                current.record.revision,
                "Reviewed as-issued identification",
            ),
        ),
    )
}

/** Only printer selection/submission is controlled; saving uses the actual desktop file adapter. */
private class ControlledOutputPort : LocalOutputPort {
    @Volatile var destination: Path? = null

    @Volatile var printResult: LocalOutputResult = LocalOutputResult.Cancelled
    val documents = CopyOnWriteArrayList<HubLocalOutput>()
    private val desktop = DesktopLocalOutputPort { destination }

    override suspend fun perform(
        action: LocalOutputAction,
        document: HubLocalOutput,
    ): LocalOutputResult {
        documents += document
        return if (action == LocalOutputAction.Print) printResult else desktop.perform(action, document)
    }

    override fun cancel() {
        desktop.cancel()
    }

    override fun close() {
        desktop.close()
    }
}

private class OutputClipboard : TextClipboard {
    override fun readText(): String? = null

    override fun writeText(value: String) = Unit
}
