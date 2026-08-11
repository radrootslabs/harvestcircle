package org.harvestcircle.ui.shell

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.ShellDestination
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class WorkspaceSidebarTest {
    @Test
    fun sidebarSelectsFoundationRoutesAndDisablesDeferredEntries() =
        runComposeUiTest {
            val selected = mutableListOf<ShellDestination>()
            setContent { WorkspaceSidebar(ShellDestination.Today, selected::add) }
            onNodeWithTag("sidebar-Today").assertIsSelected()
            onNodeWithText("Explore").assertIsNotEnabled()
            onNodeWithText("Activity").assertIsNotEnabled()
            onNodeWithText("Add a farm workspace").assertIsNotEnabled()
            onNodeWithText("Explore").performClick()
            onNodeWithText("Network").performClick()
            onNodeWithText("Settings").performClick()
            assertEquals(listOf(ShellDestination.Network, ShellDestination.Settings), selected)
        }
}
