package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Column
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class ShellControlsUiTest {
    @Test
    fun controlsExposeTargetsSelectionDisabledStateAndFieldCopy() =
        runComposeUiTest {
            setContent {
                Column {
                    ShellTab(
                        "Today",
                        "Show Today",
                        selected = true,
                        onClick = {},
                        modifier = Modifier.testTag("control-tab"),
                    )
                    ShellButton("Unavailable", "Unavailable action", {}, Modifier.testTag("control-disabled"), enabled = false)
                    ShellTextField("", {}, "Nostr reference", "npub1…", Modifier.testTag("control-field"))
                    ShellIconButton("?", "Help", {}, Modifier.testTag("control-icon"))
                }
            }

            onNodeWithTag("control-tab").assertIsSelected().assertHeightIsAtLeast(44.dp)
            onNodeWithTag("control-disabled").assertIsNotEnabled()
            onNodeWithTag("control-field").assertTextContains("npub1…")
            onNodeWithTag("control-icon").assertHeightIsAtLeast(44.dp)
        }
}
