package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Column
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
import org.harvestcircle.designsystem.component.HarvestCircleButtonVariant
import org.harvestcircle.designsystem.component.action.HarvestCircleIconButton
import org.harvestcircle.designsystem.component.action.HarvestCircleLabeledButton
import org.harvestcircle.designsystem.component.input.HarvestCircleTextField
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTab
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTabRow
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import kotlin.test.Test
import kotlin.test.assertFalse

@OptIn(ExperimentalTestApi::class)
class OwnedControlsUiTest {
    @Test
    fun hcSc010ControlsExposeTargetsSelectionDisabledStateAndFieldCopy() =
        runComposeUiTest {
            setHarvestCircleContent {
                Column {
                    HarvestCircleTabRow {
                        HarvestCircleTab(
                            label = "Today",
                            selected = true,
                            onClick = {},
                            modifier =
                                Modifier
                                    .semantics { contentDescription = "Show Today" }
                                    .testTag("control-tab"),
                        )
                    }
                    HarvestCircleLabeledButton(
                        "Unavailable",
                        "Unavailable action",
                        {},
                        Modifier.testTag("control-disabled"),
                        enabled = false,
                    )
                    HarvestCircleTextField(
                        "",
                        {},
                        label = "Nostr reference",
                        placeholder = "npub1…",
                        inputModifier = Modifier.testTag("control-field"),
                    )
                    HarvestCircleIconButton(
                        onClick = {},
                        icon = HarvestCircleIcons.Info,
                        label = "Help",
                        modifier = Modifier.testTag("control-icon"),
                    )
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
                            HarvestCircleLabeledButton(
                                "Primary",
                                "Primary action",
                                {},
                                Modifier.testTag("control-primary"),
                                variant = HarvestCircleButtonVariant.Primary,
                            )
                            HarvestCircleLabeledButton(
                                "Destructive",
                                "Destructive action",
                                {},
                                Modifier.testTag("control-destructive"),
                                variant = HarvestCircleButtonVariant.Destructive,
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
