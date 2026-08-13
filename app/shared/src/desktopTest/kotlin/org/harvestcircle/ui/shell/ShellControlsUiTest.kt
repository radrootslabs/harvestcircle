package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Column
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.ThemePreference
import kotlin.test.Test
import kotlin.test.assertFalse

@OptIn(ExperimentalTestApi::class)
class ShellControlsUiTest {
    @Test
    fun hcSc010ControlsExposeTargetsSelectionDisabledStateAndFieldCopy() =
        runComposeUiTest {
            setHarvestCircleContent {
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

            onNodeWithTag("control-tab")
                .assertIsSelected()
                .assertIsEnabled()
                .assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab))
                .assertHeightIsAtLeast(32.dp)
            onNodeWithTag("control-disabled").assertIsNotEnabled()
            assertFalse(onNodeWithTag("control-disabled").fetchSemanticsNode().config.contains(SemanticsProperties.Selected))
            onNodeWithTag("control-field").assertTextContains("npub1…")
            onNodeWithTag("control-icon").assertHeightIsAtLeast(32.dp)
        }

    @Test
    fun ownedControlsRetainActionSemanticsInLightAndDark() {
        listOf(false, true).forEach { systemDark ->
            runComposeUiTest {
                setHarvestCircleContent {
                    HarvestCircleTheme(AppearanceState(theme = ThemePreference.System), systemDark = systemDark) {
                        Column {
                            ShellButton(
                                "Primary",
                                "Primary action",
                                {},
                                Modifier.testTag("control-primary"),
                                kind = ShellButtonKind.Primary,
                            )
                            ShellButton(
                                "Destructive",
                                "Destructive action",
                                {},
                                Modifier.testTag("control-destructive"),
                                kind = ShellButtonKind.Destructive,
                            )
                        }
                    }
                }

                onNodeWithTag("control-primary").assertIsEnabled().assertTextContains("Primary")
                onNodeWithTag("control-destructive").assertIsEnabled().assertTextContains("Destructive")
            }
        }
    }
}
