package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.BuildInfo
import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.navigation.SettingsSection
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class FoundationSettingsScreenTest {
    @Test
    fun appearanceDispatchesSessionLocalTypedChoices() =
        runComposeUiTest {
            val selectedSections = mutableListOf<SettingsSection>()
            val themes = mutableListOf<ThemePreference>()
            val textSizes = mutableListOf<TextSizePreference>()
            val motions = mutableListOf<MotionPreference>()
            setContent {
                FoundationSettingsScreen(
                    SettingsSection.Appearance,
                    AppearanceState(),
                    BuildInfo.unknown(),
                    FoundationSettingsActions(selectedSections::add, themes::add, textSizes::add, motions::add),
                    HarvestCirclePlatformActions(),
                )
            }

            onNodeWithTag("bounded-detail-settings").assertExists()
            onNodeWithTag("settings-appearance").assertIsSelected().assertIsNotEnabled()
            onNodeWithTag("theme-system").assertIsSelected().assertIsNotEnabled()
            onNodeWithTag("text-size-default").assertIsSelected().assertIsNotEnabled()
            onNodeWithTag("motion-standard").assertIsSelected().assertIsNotEnabled()
            onNodeWithTag("theme-dark").performClick()
            onNodeWithTag("text-size-verylarge").performClick()
            onNodeWithTag("motion-reduced").performClick()
            onNodeWithTag("settings-project").performClick()
            assertEquals(listOf(ThemePreference.Dark), themes)
            assertEquals(listOf(TextSizePreference.VeryLarge), textSizes)
            assertEquals(listOf(MotionPreference.Reduced), motions)
            assertEquals(listOf(SettingsSection.Project), selectedSections)
        }

    @Test
    fun projectShowsActualUnknownProvenanceAndFixedTypedActions() =
        runComposeUiTest {
            var source = 0
            var licence = 0
            setContent {
                FoundationSettingsScreen(
                    SettingsSection.Project,
                    AppearanceState(),
                    BuildInfo.unknown(),
                    FoundationSettingsActions({}, {}, {}, {}),
                    HarvestCirclePlatformActions(
                        openSource = { source += 1 },
                        openLicence = { licence += 1 },
                    ),
                )
            }

            onNodeWithTag("project-source-commit").assertTextEquals("unknown")
            onNodeWithTag("project-source-state").assertTextEquals("Unknown")
            onNodeWithTag("project-ffi-contract").assertTextEquals("unknown 0.0")
            onNodeWithTag("project-storage-schema").assertTextEquals("0..0")
            onNodeWithText("GPL-3.0-only").assertExists()
            onNodeWithTag("project-open-source").performClick()
            onNodeWithTag("project-open-licence").performClick()
            assertEquals(1, source)
            assertEquals(1, licence)
            onAllNodesWithText("Locality").assertCountEquals(0)
            onAllNodesWithText("Delete data").assertCountEquals(0)
        }

    @Test
    fun veryLargeProjectFactsRemainReachableInTheBoundedPane() =
        runComposeUiTest {
            setContent {
                Box(Modifier.size(640.dp, 360.dp)) {
                    HarvestCircleTheme(AppearanceState(textSize = TextSizePreference.VeryLarge)) {
                        FoundationSettingsScreen(
                            SettingsSection.Project,
                            AppearanceState(textSize = TextSizePreference.VeryLarge),
                            BuildInfo.unknown(),
                            FoundationSettingsActions({}, {}, {}, {}),
                            HarvestCirclePlatformActions(),
                        )
                    }
                }
            }

            onNodeWithTag("project-storage-schema").performScrollTo().assertIsDisplayed()
        }
}
