package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Column
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.HarvestCircleDesign
import org.harvestcircle.design.ThemePreference
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

    @Test
    fun renderedSemanticColorsMatchThePureResolverInLightAndDark() {
        listOf(false, true).forEach { systemDark ->
            val palette = if (systemDark) HarvestCircleDesign.dark else HarvestCircleDesign.light
            runComposeUiTest {
                setContent {
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

                val primary = resolveShellControlVisuals(ShellButtonKind.Primary, true, false, false, false, false, palette)
                val destructive =
                    resolveShellControlVisuals(ShellButtonKind.Destructive, true, false, false, false, false, palette)
                onNodeWithTag("control-primary")
                    .assert(SemanticsMatcher.expectValue(ShellControlBackgroundKey, primary.background.hexValue()))
                    .assert(SemanticsMatcher.expectValue(ShellControlForegroundKey, primary.foreground.hex))
                    .assert(SemanticsMatcher.expectValue(ShellControlBorderKey, primary.border.hex))
                onNodeWithTag("control-destructive")
                    .assert(SemanticsMatcher.expectValue(ShellControlBackgroundKey, destructive.background.hexValue()))
                    .assert(SemanticsMatcher.expectValue(ShellControlForegroundKey, destructive.foreground.hex))
                    .assert(SemanticsMatcher.expectValue(ShellControlBorderKey, destructive.border.hex))
            }
        }
    }
}

private fun ShellControlBackground.hexValue(): String =
    when (this) {
        is ShellControlBackground.Solid -> color.hex
        ShellControlBackground.Transparent -> "transparent"
    }
