package org.harvestcircle.ui.shell

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
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
            onNodeWithText("Explore").assertIsNotEnabled().assertIsNotSelected()
            onNodeWithText("Activity").assertIsNotEnabled().assertIsNotSelected()
            onNodeWithText("Add a farm workspace").assertIsNotEnabled()
            onNodeWithContentDescription("Explore. Not available in this build.").assertExists()
            onNodeWithContentDescription("Activity. Not available in this build.").assertExists()
            onNodeWithContentDescription("Add a farm workspace. Not available in this build.").assertExists()
            onNodeWithText("Explore").performClick()
            onNodeWithText("Network").performClick()
            onNodeWithText("Settings").performClick()
            assertTrue(
                onNodeWithTag("sidebar-Settings").fetchSemanticsNode().boundsInRoot.top >
                    onNodeWithTag("sidebar-add-farm").fetchSemanticsNode().boundsInRoot.bottom,
            )
            assertEquals(listOf(ScreenKey.Network, ScreenKey.Settings), selected)
        }
}
