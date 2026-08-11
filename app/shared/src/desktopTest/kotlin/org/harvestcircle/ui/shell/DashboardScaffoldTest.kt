package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class DashboardScaffoldTest {
    @Test
    fun preferredWindowKeepsFixedRegionsAndInspectorBeside() =
        runComposeUiTest {
            setContent { dashboard(width = 1280) }
            onNodeWithTag("dashboard-top-bar").assertIsDisplayed()
            onNodeWithTag("dashboard-sidebar").assertIsDisplayed()
            onNodeWithTag("dashboard-main-header").assertIsDisplayed()
            onNodeWithTag("dashboard-main-body").assertIsDisplayed()
            onNodeWithTag("dashboard-inspector-beside").assertIsDisplayed()
        }

    @Test
    fun minimumWindowMovesInspectorToOverlay() =
        runComposeUiTest {
            setContent { dashboard(width = 1100) }
            onNodeWithTag("dashboard-inspector-overlay").assertIsDisplayed()
            onNodeWithTag("dashboard-main-body").assertIsDisplayed()
        }
}

@androidx.compose.runtime.Composable
private fun dashboard(width: Int) {
    Box(Modifier.requiredSize(width.dp, 720.dp)) {
        DashboardScaffold(
            windowWidthDp = width,
            inspectorVisible = true,
            topBar = {},
            sidebar = {},
            mainHeader = {},
            mainBody = {},
            inspector = {},
        )
    }
}
