package org.harvestcircle.ui.shell

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class MainPanelTemplatesTest {
    @Test
    fun masterDetailOwnsItsInternalListScrollAndTypedSelection() =
        runComposeUiTest {
            val selection = TemplateSelectionKey("selected")
            var observed: TemplateSelectionKey? = null
            setContent { MasterDetailTemplate(selection, master = {}, detail = { observed = it }) }
            assertEquals(selection, observed)
            assertTrue(onNodeWithTag("template-master-list").fetchSemanticsNode().config.contains(SemanticsActions.ScrollBy))
        }

    @Test
    fun tabbedAndStudioTemplatesExposeOnlyStructuralSlots() =
        runComposeUiTest {
            val tab = TemplateTab(TemplateSelectionKey("overview"), "Overview")
            setContent {
                TabbedDetailTemplate(listOf(tab), tab.key, tabRail = { _, _ -> }, detail = {})
                StudioTemplate(rail = {}, body = {}, action = {})
            }
            onAllNodesWithTag("template-tabs").assertCountEquals(1)
            onAllNodesWithTag("template-workbench-rail").assertCountEquals(1)
            onAllNodesWithTag("template-workbench-action").assertCountEquals(1)
        }
}
