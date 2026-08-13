package org.harvestcircle.ui.shell

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class MainPanelHeaderTest {
    @Test
    fun headerExposesTitleBreadcrumbStatusTabsAndActionSlots() =
        runComposeUiTest {
            val overview = TemplateTab(TemplateSelectionKey("overview"), "Overview")
            val identity = TemplateTab(TemplateSelectionKey("identity"), "Identity")
            val selections = mutableListOf<TemplateSelectionKey>()
            setHarvestCircleContent {
                MainPanelHeader(
                    MainPanelHeaderModel(
                        title = "Network",
                        breadcrumb = listOf("Personal", "Network"),
                        localStatus = "Limited connection",
                        tabs = listOf(overview, identity),
                        selectedTab = overview.key,
                    ),
                    onTabSelected = selections::add,
                )
            }
            onNodeWithText("Network").assertIsDisplayed()
            onNodeWithTag("main-breadcrumb").assertIsDisplayed()
            onNodeWithTag("main-tab-overview").assertIsSelected().assertIsEnabled().performClick()
            onNodeWithTag("main-tab-identity").performClick()
            onAllNodesWithTag("main-primary-action").assertCountEquals(1)
            assertEquals(listOf(identity.key), selections)
        }

    @Test
    fun tabFootprintsRemainStableWhenSelectionChanges() =
        runComposeUiTest {
            val relays = TemplateTab(TemplateSelectionKey("public-relays"), "Public relays")
            val diagnostics = TemplateTab(TemplateSelectionKey("runtime-diagnostics"), "Runtime diagnostics")
            var selected by mutableStateOf(relays.key)
            setHarvestCircleContent {
                MainPanelHeader(
                    MainPanelHeaderModel(
                        title = "Network",
                        tabs = listOf(relays, diagnostics),
                        selectedTab = selected,
                    ),
                    onTabSelected = { selected = it },
                )
            }

            val relaysBefore = onNodeWithTag("main-tab-public-relays").fetchSemanticsNode().boundsInRoot
            val diagnosticsBefore = onNodeWithTag("main-tab-runtime-diagnostics").fetchSemanticsNode().boundsInRoot
            onNodeWithTag("main-tab-runtime-diagnostics").performClick()
            waitForIdle()

            assertEquals(relaysBefore, onNodeWithTag("main-tab-public-relays").fetchSemanticsNode().boundsInRoot)
            assertEquals(diagnosticsBefore, onNodeWithTag("main-tab-runtime-diagnostics").fetchSemanticsNode().boundsInRoot)
        }
}
