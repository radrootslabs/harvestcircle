package org.harvestcircle.integration

import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.printToString
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.ApplicationClock
import org.harvestcircle.application.HarvestCircleApplicationWithDependencies
import org.harvestcircle.application.HarvestCirclePresenter
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.SecretClipboardController
import org.harvestcircle.application.TextClipboard
import org.harvestcircle.application.UnixSeconds
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.readBytes
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class IdentityBootstrapAcceptanceTest {
    @Test
    fun semanticUiFlowCreatesActivatesClosesAndReopensPersistedIdentity() =
        runComposeUiTest {
            val dataRoot = Files.createTempDirectory("harvestcircle-acceptance-")
            val runtime = TestBridgeHarvestCircleRuntime.open(dataRoot.toString())
            val clipboard = RecordingClipboard()
            val operationIds = SequentialOperationIds()
            var showApplication by mutableStateOf(true)
            var closeRequested by mutableStateOf(false)
            var applicationSession by mutableStateOf(0)
            var approvedExits = 0
            try {
                setContent {
                    if (showApplication) {
                        key(applicationSession) {
                            HarvestCircleApplicationWithDependencies(
                                closeRequested = closeRequested,
                                onExitApproved = { approvedExits += 1 },
                                clipboardFactory = { scope ->
                                    SecretClipboardController(
                                        scope = scope,
                                        clipboard = clipboard,
                                        clearDelayMillis = 60_000,
                                    )
                                },
                            ) { scope ->
                                HarvestCirclePresenter(
                                    runtime = runtime,
                                    scope = scope,
                                    clock = ApplicationClock { UnixSeconds(FIXED_TIME_SECONDS) },
                                    operationIds = operationIds,
                                )
                            }
                        }
                    }
                }

                waitForTag("bootstrap-welcome")
                onNodeWithTag("bootstrap-create").performClick()
                onNodeWithTag("generate-key").performClick()
                waitForTag("generated-key-backup")
                onNodeWithTag("generated-nsec").assertIsDisplayed()
                onNodeWithTag("copy-generated-key").performClick()
                val copiedSecret = assertNotNull(clipboard.readText())
                assertTrue(copiedSecret.startsWith("nsec1"))

                onNodeWithTag("acknowledge-key-backup").performClick()
                waitForTag("saved-identity-list")
                onAllNodesWithTag("generated-key-backup").assertCountEquals(0)
                onAllNodesWithText(copiedSecret).assertCountEquals(0)

                runtime.seedSelectedProfile("Farm Identity")
                onNodeWithText("Activate identity").performClick()
                waitForTag("foundation-today")
                onNodeWithTag("sidebar-Network").performClick()
                waitForTag("network-overview")
                onNodeWithTag("network-tab-identity").performClick()
                onNodeWithTag("refresh-profile").performClick()
                waitUntil(timeoutMillis = UI_TIMEOUT_MILLIS) {
                    onAllNodesWithText("Display name: Farm Identity").fetchSemanticsNodes().size == 1
                }
                onNodeWithText("Display name: Farm Identity").assertIsDisplayed()

                onNodeWithTag("sign-out").performClick()
                waitForTag("saved-identity-list")
                onNodeWithText("Activate identity").performClick()
                waitForTag("foundation-today")
                assertFalse(onRoot(useUnmergedTree = true).printToString().contains(copiedSecret))
                closeRequested = true
                waitUntil(timeoutMillis = UI_TIMEOUT_MILLIS) { approvedExits == 1 }
                assertNull(clipboard.readText())

                showApplication = false
                waitForIdle()
                runtime.restart()
                applicationSession += 1
                closeRequested = false
                showApplication = true

                waitForTag("saved-identity-list")
                onNodeWithText("Activate identity").performClick()
                waitForTag("foundation-today")
                onNodeWithText("No active commitments").assertIsDisplayed()
                onAllNodesWithText(copiedSecret).assertCountEquals(0)

                assertFalse(runtime.currentSnapshot().toString().contains(copiedSecret))
                closeRequested = true
                waitUntil(timeoutMillis = UI_TIMEOUT_MILLIS) { approvedExits == 2 }
                showApplication = false
                waitForIdle()

                val secretBytes = copiedSecret.encodeToByteArray()
                try {
                    Files.walk(dataRoot).use { paths ->
                        paths
                            .filter(Files::isRegularFile)
                            .forEach { file -> assertFalse(file.readBytes().containsBytes(secretBytes)) }
                    }
                } finally {
                    secretBytes.fill(0)
                }
            } finally {
                runtime.close()
                deleteAcceptanceTree(dataRoot)
            }
        }

    @Test
    fun readOnlySessionRestartsAtBootstrapWithoutPersistingAuthority() =
        runComposeUiTest {
            val dataRoot = Files.createTempDirectory("harvestcircle-read-only-")
            val runtime = TestBridgeHarvestCircleRuntime.open(dataRoot.toString())
            var showApplication by mutableStateOf(true)
            var closeRequested by mutableStateOf(false)
            var applicationSession by mutableStateOf(0)
            var approvedExits = 0
            try {
                setContent {
                    if (showApplication) {
                        key(applicationSession) {
                            HarvestCircleApplicationWithDependencies(
                                closeRequested = closeRequested,
                                onExitApproved = { approvedExits += 1 },
                                clipboardFactory = { scope ->
                                    SecretClipboardController(
                                        scope = scope,
                                        clipboard = RecordingClipboard(),
                                        clearDelayMillis = 60_000,
                                    )
                                },
                            ) { scope ->
                                HarvestCirclePresenter(
                                    runtime = runtime,
                                    scope = scope,
                                    clock = ApplicationClock { UnixSeconds(FIXED_TIME_SECONDS) },
                                    operationIds = SequentialOperationIds(),
                                )
                            }
                        }
                    }
                }

                waitForTag("bootstrap-welcome")
                onNodeWithTag("bootstrap-read-only").performClick()
                waitForTag("foundation-today")
                onNodeWithText("Read-only session").assertIsDisplayed()
                assertTrue(runtime.currentSnapshot().identities.isEmpty())

                closeRequested = true
                waitUntil(timeoutMillis = UI_TIMEOUT_MILLIS) { approvedExits == 1 }
                showApplication = false
                waitForIdle()
                runtime.restart()
                applicationSession += 1
                closeRequested = false
                showApplication = true

                waitForTag("bootstrap-welcome")
                onAllNodesWithTag("foundation-today").assertCountEquals(0)
                assertTrue(runtime.currentSnapshot().identities.isEmpty())
                closeRequested = true
                waitUntil(timeoutMillis = UI_TIMEOUT_MILLIS) { approvedExits == 2 }
                showApplication = false
                waitForIdle()
            } finally {
                runtime.close()
                deleteAcceptanceTree(dataRoot)
            }
        }

    private fun androidx.compose.ui.test.ComposeUiTest.waitForTag(tag: String) {
        try {
            waitUntil(timeoutMillis = UI_TIMEOUT_MILLIS) {
                onAllNodesWithTag(tag).fetchSemanticsNodes().size == 1
            }
        } catch (error: androidx.compose.ui.test.ComposeTimeoutException) {
            throw AssertionError("Timed out waiting for semantic tag: $tag\n${onRoot().printToString()}", error)
        }
    }
}

private class RecordingClipboard : TextClipboard {
    private var value: String? = null

    override fun readText(): String? = value

    override fun writeText(value: String) {
        this.value = value.ifEmpty { null }
    }
}

private class SequentialOperationIds : OperationIdSource {
    private var next = 1

    override fun next(): OperationId = OperationId.from("00000000-0000-7000-8000-${next++.toString().padStart(12, '0')}")
}

private fun ByteArray.containsBytes(needle: ByteArray): Boolean =
    needle.isNotEmpty() &&
        indices.any { start ->
            start + needle.size <= size && needle.indices.all { offset -> this[start + offset] == needle[offset] }
        }

private fun deleteAcceptanceTree(root: Path) {
    Files.walk(root).use { paths ->
        paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists)
    }
}

private const val FIXED_TIME_SECONDS = 1_700_000_000L
private const val UI_TIMEOUT_MILLIS = 10_000L
