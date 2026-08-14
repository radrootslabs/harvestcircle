package org.harvestcircle.hostui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ComposeUiTest
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.printToString
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationClock
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.HarvestCircleApplicationWithDependencies
import org.harvestcircle.application.HarvestCirclePresenter
import org.harvestcircle.application.NativeHarvestCircleRuntime
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.RequestContext
import org.harvestcircle.application.SecretClipboardController
import org.harvestcircle.application.TextClipboard
import org.harvestcircle.application.UnixSeconds
import org.harvestcircle.application.desktopRuntimeOpenConfiguration
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeEnvironment
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeExclusion
import org.harvestcircle.desktop.desktopWindowChromeExclusion
import java.nio.file.Files
import java.nio.file.Path
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class IdentityLifecycleHostUiTest {
    @Test
    fun semanticUiGeneratesAddsActivatesAndRemovesIdentityThroughProductionNativeRuntime() =
        runComposeUiTest {
            val dataRoot = Files.createTempDirectory("harvestcircle-host-ui-").toRealPath()
            val runtime =
                NativeHarvestCircleRuntime.open(
                    desktopRuntimeOpenConfiguration(
                        developmentMode = true,
                        explicitDataDirectory = dataRoot.toString(),
                        configuredRelays = "",
                    ),
                )
            val operationIds = HostOperationIds()
            lateinit var presenter: HarvestCirclePresenter
            var closeRequested by mutableStateOf(false)
            var approvedExits = 0
            val windowChromeExclusion = desktopWindowChromeExclusion(hostIsMacOs())
            try {
                setContent {
                    HarvestCircleWindowChromeEnvironment(windowChromeExclusion) {
                        HarvestCircleApplicationWithDependencies(
                            closeRequested = closeRequested,
                            onExitApproved = { approvedExits += 1 },
                            clipboardFactory = { scope ->
                                SecretClipboardController(scope, EmptyClipboard(), clearDelayMillis = 60_000)
                            },
                        ) { scope ->
                            HarvestCirclePresenter(
                                runtime = runtime,
                                scope = scope,
                                clock = ApplicationClock { UnixSeconds(System.currentTimeMillis() / 1_000) },
                                operationIds = operationIds,
                            ).also { presenter = it }
                        }
                    }
                }

                waitForTag("bootstrap-welcome")
                assertCanvasChromeClearance(windowChromeExclusion)
                onNodeWithTag("bootstrap-read-only").performClick()
                waitForTag("foundation-today")
                assertDashboardChromeClearance(windowChromeExclusion)
                onNodeWithTag("top-bar-signer").performClick()
                waitForTag("signer-add-or-activate-identity")
                onNodeWithTag("signer-add-or-activate-identity").performClick()
                waitForTag("bootstrap-welcome")
                onNodeWithTag("bootstrap-create").performClick()
                onNodeWithTag("generate-key").performClick()
                waitForTag("generated-key-backup")
                onNodeWithTag("acknowledge-key-backup").performClick()
                waitForTag("saved-identity-list")

                val added = runtime.currentSnapshot()
                assertEquals(1, added.identities.size)
                val identityId = assertNotNull(added.selectedIdentityId)

                onNodeWithTag("activate-identity:${identityId.value}").performClick()
                try {
                    waitForTag("foundation-today")
                } catch (error: AssertionError) {
                    throw AssertionError(
                        "Activation state: ${presenter.state.value}",
                        error,
                    )
                }
                val activated = runtime.currentSnapshot()
                assertEquals(identityId, activated.activeIdentity?.identity?.id)

                onNodeWithTag("sidebar-Network").performClick()
                waitForTag("network-overview")
                onNodeWithTag("main-tab-identity").performClick()
                onNodeWithTag("sign-out").performClick()
                waitForTag("saved-identity-list")
                onNodeWithTag("remove-identity:${identityId.value}").performClick()
                waitForEnabled("overlay-confirm")
                onNodeWithTag("overlay-confirm").performClick()
                waitForTag("bootstrap-welcome")

                val removed = runtime.currentSnapshot()
                assertTrue(removed.identities.isEmpty())
                assertEquals(null, removed.activeIdentity)
                onAllNodesWithTag("identity-row:${identityId.value}").assertCountEquals(0)

                closeRequested = true
                waitUntil(timeoutMillis = UI_TIMEOUT_MILLIS) { approvedExits == 1 }
            } finally {
                cleanupRuntime(runtime, operationIds)
                deleteTree(dataRoot)
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
        onNodeWithTag(tag).assertIsDisplayed()
    }

    private fun androidx.compose.ui.test.ComposeUiTest.waitForEnabled(tag: String) {
        waitUntil(timeoutMillis = UI_TIMEOUT_MILLIS) {
            runCatching { onNodeWithTag(tag).assertIsEnabled() }.isSuccess
        }
    }

    private fun ComposeUiTest.assertCanvasChromeClearance(exclusion: HarvestCircleWindowChromeExclusion) {
        if (exclusion == HarvestCircleWindowChromeExclusion.None) return
        val canvas = onNodeWithTag("canvas-scaffold").fetchSemanticsNode().boundsInRoot
        val content = onNodeWithTag("harvestcircle-canvas-chrome-content").fetchSemanticsNode().boundsInRoot
        val exclusionWidth = with(density) { exclusion.width.toPx() }
        assertTrue(content.left >= canvas.left + exclusionWidth)
    }

    private fun ComposeUiTest.assertDashboardChromeClearance(exclusion: HarvestCircleWindowChromeExclusion) {
        if (exclusion == HarvestCircleWindowChromeExclusion.None) return
        val frame = onNodeWithTag("harvestcircle-frame").fetchSemanticsNode().boundsInRoot
        val toggle = onNodeWithTag("workspace-sidebar-toggle").fetchSemanticsNode().boundsInRoot
        val exclusionWidth = with(density) { exclusion.width.toPx() }
        assertTrue(toggle.left >= frame.left + exclusionWidth)
        onAllNodesWithTag("top-bar-sidebar-toggle").assertCountEquals(0)
    }
}

private fun cleanupRuntime(
    runtime: NativeHarvestCircleRuntime,
    operationIds: OperationIdSource,
) = runBlocking {
    runCatching { runtime.execute(ApplicationCommand.SignOut) }
    runCatching {
        runtime.currentSnapshot().identities.forEach { identity ->
            val request = runtime.requestIdentityRemoval(identity.id)
            val revision = runtime.currentSnapshot().revision
            runtime.execute(
                ApplicationCommand.ConfirmIdentityRemoval(
                    request.requestId,
                    RequestContext(operationIds.next(), revision, 2_000UL),
                ),
            )
        }
    }
    runCatching { runtime.shutdown() }
}

private class HostOperationIds : OperationIdSource {
    private var next = 1

    override fun next(): OperationId = OperationId.from("00000000-0000-7000-8001-${next++.toString().padStart(12, '0')}")
}

private class EmptyClipboard : TextClipboard {
    override fun readText(): String? = null

    override fun writeText(value: String) = Unit
}

private fun deleteTree(root: Path) {
    Files.walk(root).use { paths ->
        paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists)
    }
}

private const val UI_TIMEOUT_MILLIS = 15_000L

private fun hostIsMacOs(): Boolean =
    System
        .getProperty("os.name", "")
        .startsWith("Mac", ignoreCase = true)
