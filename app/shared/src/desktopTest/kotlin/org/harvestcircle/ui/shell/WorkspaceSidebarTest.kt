package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeClearance
import org.harvestcircle.product.ScreenKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class WorkspaceSidebarTest {
    @Test
    fun sidebarSelectsFoundationRoutesAndDisablesDeferredEntries() =
        runComposeUiTest {
            val selected = mutableListOf<ScreenKey>()
            setHarvestCircleContent { WorkspaceSidebar(ScreenKey.PersonalToday, selected::add) }
            onNodeWithTag("sidebar-PersonalToday").assertIsSelected().assertIsEnabled().performClick()
            onNodeWithTag("sidebar-Explore").assertIsNotEnabled()
            onNodeWithTag("sidebar-Activity").assertIsNotEnabled()
            onNodeWithTag("sidebar-add-farm").assertIsNotEnabled()
            onNodeWithContentDescription("Explore. Not available in this build.").assertExists()
            onNodeWithContentDescription("Activity. Not available in this build.").assertExists()
            onNodeWithContentDescription("Add a farm workspace. Not available in this build.").assertExists()
            onNodeWithTag("sidebar-Explore").performClick()
            onNodeWithText("Network").performClick()
            onNodeWithText("Settings").performClick()
            assertTrue(
                onNodeWithTag("sidebar-Settings").fetchSemanticsNode().boundsInRoot.top >
                    onNodeWithTag("sidebar-add-farm").fetchSemanticsNode().boundsInRoot.bottom,
            )
            assertEquals(listOf(ScreenKey.Network, ScreenKey.Settings), selected)
        }

    @Test
    fun hostChromeHidesRedundantBrandingAndKeepsTheCollapseControlSafe() =
        runComposeUiTest {
            setHarvestCircleContent {
                Box(Modifier.requiredSize(232.dp, 720.dp)) {
                    WorkspaceSidebar(
                        selected = ScreenKey.PersonalToday,
                        onScreen = {},
                        chromeClearance =
                            HarvestCircleWindowChromeClearance(
                                topBandHeight = 48.dp,
                                left = 112.dp,
                                right = 0.dp,
                            ),
                    )
                }
            }

            onAllNodesWithText("HarvestCircle").assertCountEquals(0)
            assertTrue(onNodeWithTag("workspace-sidebar-toggle").fetchSemanticsNode().boundsInRoot.left >= 112f)
        }
}
