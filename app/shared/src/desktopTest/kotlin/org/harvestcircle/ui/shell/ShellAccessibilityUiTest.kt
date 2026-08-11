package org.harvestcircle.ui.shell

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.onNodeWithTag
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
}
