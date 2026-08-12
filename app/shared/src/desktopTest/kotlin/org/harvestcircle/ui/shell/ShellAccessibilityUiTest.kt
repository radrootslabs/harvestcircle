package org.harvestcircle.ui.shell

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.application.ApplicationSnapshot
import org.harvestcircle.application.BuildInfo
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.HarvestCirclePresenterState
import org.harvestcircle.application.HarvestCircleShellState
import org.harvestcircle.application.OverlayState
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.application.StatusOverlayKey
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class ShellAccessibilityUiTest {
    @Test
    fun sharedActionsMeetTheMinimumTargetAndRouteContentRestoresFocus() =
        runComposeUiTest {
            setContent {
                RouteFocusTarget("today", "Today main content") {
                    ShellAction("Action", "Accessible action", "accessible-action", onClick = {})
                }
            }

            onNodeWithTag("accessible-action").assertHeightIsAtLeast(44.dp)
            onNodeWithTag("route-focus-target").assertIsFocused()
        }

    @Test
    fun keyboardHostDispatchesModifiedAndEscapeShortcuts() =
        runComposeUiTest {
            val shortcuts = mutableListOf<ShellShortcut>()
            setContent {
                ShellKeyboardHost(onShortcut = shortcuts::add) {
                    RouteFocusTarget("today", "Today main content", content = {})
                }
            }

            onNodeWithTag("route-focus-target").performKeyInput {
                keyDown(Key.CtrlLeft)
                keyDown(Key.K)
                keyUp(Key.K)
                keyUp(Key.CtrlLeft)
                keyDown(Key.Escape)
                keyUp(Key.Escape)
            }
            kotlin.test.assertEquals(
                listOf(ShellShortcut.OpenNostrReference, ShellShortcut.CloseOverlay),
                shortcuts,
            )
        }

    @Test
    fun keyboardHostSuppressesBackgroundShortcutsWhileModalIsOpen() =
        runComposeUiTest {
            val shortcuts = mutableListOf<ShellShortcut>()
            setContent {
                ShellKeyboardHost(
                    modal = FoundationOverlay.Status(StatusOverlayKey.Sync),
                    onShortcut = shortcuts::add,
                ) {
                    RouteFocusTarget("today", "Today main content", content = {})
                }
            }

            onNodeWithTag("route-focus-target").performKeyInput {
                keyDown(Key.CtrlLeft)
                keyDown(Key.K)
                keyUp(Key.K)
                keyUp(Key.CtrlLeft)
                keyDown(Key.Escape)
                keyUp(Key.Escape)
            }
            kotlin.test.assertEquals(listOf(ShellShortcut.CloseOverlay), shortcuts)
        }

    @Test
    fun hcSc011ModalRemovesTheBackgroundSemanticsSubtree() =
        runComposeUiTest {
            setContent {
                HarvestCircleShell(
                    state = modalBootstrapState(),
                    identityActions = HarvestCircleUiActions(),
                    platformActions = HarvestCirclePlatformActions(),
                    dispatch = {},
                )
            }

            onAllNodesWithTag("bootstrap-welcome").assertCountEquals(0)
            onAllNodesWithTag("foundation-overlay").assertCountEquals(1)
        }

    @Test
    fun routeTargetRestoresFocusAfterAClosedModal() =
        runComposeUiTest {
            var modalOpen by mutableStateOf(false)
            var restoreTarget by mutableStateOf<ShellFocusTarget?>(null)
            setContent {
                val registry = remember { ShellFocusRegistry() }
                CompositionLocalProvider(LocalShellFocusRegistry provides registry) {
                    RouteFocusTarget("today", "Today main content") {
                        ShellAction(
                            "Open",
                            "Open dialog",
                            "restore-trigger",
                            modifier = Modifier.shellFocusTarget(ShellFocusTarget.TodayReference),
                        ) {
                            restoreTarget = null
                            modalOpen = true
                        }
                        if (modalOpen) {
                            FoundationOverlayHost(
                                org.harvestcircle.application.OverlayState(
                                    org.harvestcircle.application.FoundationOverlay.Status(
                                        org.harvestcircle.application.StatusOverlayKey.Sync,
                                    ),
                                ),
                                org.harvestcircle.application.ShellStatusModel(
                                    org.harvestcircle.application.SyncStatusLabel.NotYetObserved,
                                    org.harvestcircle.application.SignerStatusLabel.SignedOut,
                                    null,
                                ),
                            ) {
                                modalOpen = false
                                restoreTarget = ShellFocusTarget.TodayReference
                            }
                        }
                    }
                    ShellFocusRestorer(restoreTarget, ShellFocusTarget.RouteFallback)
                }
            }

            onNodeWithTag("restore-trigger").performClick()
            onNodeWithTag("overlay-close").assertIsFocused().performClick()
            onNodeWithTag("restore-trigger").assertIsFocused()
        }
}

private fun modalBootstrapState(): HarvestCircleShellState =
    HarvestCircleShellState(
        identity =
            HarvestCirclePresenterState(
                ApplicationSnapshot(
                    revision = SnapshotRevision(1UL),
                    lifecycle = ApplicationLifecycle.Ready,
                    lifecycleProblem = null,
                    configuredRelays = emptyList(),
                    identities = emptyList(),
                    selectedIdentityId = null,
                    session = SessionLifecycle.SignedOut,
                    sessionSubjectIdentityId = null,
                    sessionProblem = null,
                    activeIdentity = null,
                    recoverableProblem = null,
                ),
            ),
        buildInfo = BuildInfo.unknown(),
        overlays = OverlayState(current = FoundationOverlay.Status(StatusOverlayKey.Sync)),
    )
