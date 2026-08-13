package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class DashboardScaffoldTest {
    @Test
    fun preferredWindowKeepsFixedRegionsAndInspectorBeside() =
        runComposeUiTest {
            setHarvestCircleContent { dashboard(width = 1280) }
            onNodeWithTag("dashboard-top-bar").assertIsDisplayed()
            onNodeWithTag("dashboard-sidebar").assertIsDisplayed()
            onNodeWithTag("dashboard-main-header").assertIsDisplayed()
            onNodeWithTag("dashboard-main-body").assertIsDisplayed()
            onNodeWithTag("dashboard-inspector-beside").assertIsDisplayed()
        }

    @Test
    fun minimumWindowMovesInspectorToOverlay() =
        runComposeUiTest {
            setHarvestCircleContent { dashboard(width = 1100) }
            onNodeWithTag("dashboard-inspector-overlay").assertIsDisplayed()
            onNodeWithTag("dashboard-main-body").assertIsDisplayed()
        }

    @Test
    fun liveConstraintChangesMoveTheInspectorWithoutRecreatingTheShell() =
        runComposeUiTest {
            var width by mutableStateOf(1280)
            setHarvestCircleContent { dashboard(width) }
            onAllNodesWithTag("dashboard-inspector-beside").assertCountEquals(1)

            width = 1100
            waitForIdle()
            onAllNodesWithTag("dashboard-inspector-beside").assertCountEquals(0)
            onAllNodesWithTag("dashboard-inspector-overlay").assertCountEquals(1)
        }
}

@androidx.compose.runtime.Composable
private fun dashboard(width: Int) {
    Box(Modifier.requiredSize(width.dp, 720.dp)) {
        DashboardScaffold(
            inspectorVisible = true,
            topBar = {},
            sidebar = {},
            mainHeader = {},
            mainBody = {},
            inspector = {},
        )
    }
}
