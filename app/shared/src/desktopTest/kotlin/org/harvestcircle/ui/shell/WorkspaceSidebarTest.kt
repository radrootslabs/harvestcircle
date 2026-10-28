package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.onAllNodesWithText
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
    fun sidebarExposesOperationalRoutesAndExistingUtilities() =
        runComposeUiTest {
            val selected = mutableListOf<ScreenKey>()
            setHarvestCircleContent { WorkspaceSidebar(ScreenKey.HubWork, selected::add) }
            onNodeWithTag("sidebar-HubWork").assertIsSelected().assertIsEnabled().performClick()
            onNodeWithTag("sidebar-HubStock").assertIsEnabled().performClick()
            onNodeWithTag("sidebar-HubDispatch").assertIsEnabled().performClick()
            onNodeWithText("Network").performClick()
            onNodeWithText("Settings").performClick()
            assertTrue(
                onNodeWithTag("sidebar-Settings").fetchSemanticsNode().boundsInRoot.top >
                    onNodeWithTag("sidebar-HubWork").fetchSemanticsNode().boundsInRoot.bottom,
            )
            assertEquals(listOf(ScreenKey.HubStock, ScreenKey.HubDispatch, ScreenKey.Network, ScreenKey.Settings), selected)
        }

    @Test
    fun hostChromeHidesRedundantBrandingAndKeepsTheCollapseControlSafe() =
        runComposeUiTest {
            setHarvestCircleContent {
                Box(Modifier.requiredSize(232.dp, 720.dp)) {
                    WorkspaceSidebar(
                        selected = ScreenKey.HubWork,
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
