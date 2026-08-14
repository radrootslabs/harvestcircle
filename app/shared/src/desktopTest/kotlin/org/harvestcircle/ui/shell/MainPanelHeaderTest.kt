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
    fun tabFootprintsAndLabelAlignmentRemainStableWhenSelectionChanges() =
        runComposeUiTest {
            val overview = TemplateTab(TemplateSelectionKey("overview"), "Overview")
            val identity = TemplateTab(TemplateSelectionKey("identity"), "Identity")
            var selected by mutableStateOf(overview.key)
            setHarvestCircleContent {
                MainPanelHeader(
                    MainPanelHeaderModel(
                        title = "Network",
                        tabs = listOf(overview, identity),
                        selectedTab = selected,
                    ),
                    onTabSelected = { selected = it },
                )
            }

            val overviewBefore = onNodeWithTag("main-tab-overview").fetchSemanticsNode().boundsInRoot
            val identityBefore = onNodeWithTag("main-tab-identity").fetchSemanticsNode().boundsInRoot
            val overviewLabelBefore = onNodeWithText("Overview", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            val identityLabelBefore = onNodeWithText("Identity", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            assertEquals(overviewBefore.center.x, overviewLabelBefore.center.x, absoluteTolerance = 0.5f)
            assertEquals(identityBefore.center.x, identityLabelBefore.center.x, absoluteTolerance = 0.5f)

            onNodeWithTag("main-tab-identity").performClick()
            waitForIdle()

            val overviewAfter = onNodeWithTag("main-tab-overview").fetchSemanticsNode().boundsInRoot
            val identityAfter = onNodeWithTag("main-tab-identity").fetchSemanticsNode().boundsInRoot
            val overviewLabelAfter = onNodeWithText("Overview", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            val identityLabelAfter = onNodeWithText("Identity", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            assertEquals(overviewBefore, overviewAfter)
            assertEquals(identityBefore, identityAfter)
            assertEquals(overviewAfter.center.x, overviewLabelAfter.center.x, absoluteTolerance = 0.5f)
            assertEquals(identityAfter.center.x, identityLabelAfter.center.x, absoluteTolerance = 0.5f)
        }
}
