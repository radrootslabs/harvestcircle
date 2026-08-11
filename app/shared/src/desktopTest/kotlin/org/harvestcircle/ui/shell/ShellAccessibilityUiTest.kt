package org.harvestcircle.ui.shell

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
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
                ShellKeyboardHost(shortcuts::add) {
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
    fun routeTargetRestoresFocusAfterAClosedModal() =
        runComposeUiTest {
            var modalOpen by mutableStateOf(false)
            setContent {
                RouteFocusTarget("today", "Today main content", restoreFocus = !modalOpen) {
                    ShellAction("Open", "Open dialog", "restore-trigger") { modalOpen = true }
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
                        ) { modalOpen = false }
                    }
                }
            }

            onNodeWithTag("restore-trigger").performClick()
            onNodeWithTag("overlay-close").assertIsFocused().performClick()
            onNodeWithTag("route-focus-target").assertIsFocused()
        }
}
