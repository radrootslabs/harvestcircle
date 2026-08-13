package org.harvestcircle.ui.shell

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class MainPanelTemplatesTest {
    @Test
    fun tabbedDetailTemplateExposesOnlyProductOwnedStructuralSlots() =
        runComposeUiTest {
            val tab = TemplateTab(TemplateSelectionKey("overview"), "Overview")
            setHarvestCircleContent {
                TabbedDetailTemplate(listOf(tab), tab.key, tabRail = { _, _ -> }, detail = {})
            }
            onAllNodesWithTag("template-tabs").assertCountEquals(1)
            onAllNodesWithTag("template-tab-detail").assertCountEquals(1)
        }
}
