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
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.SignerStatusLabel
import org.harvestcircle.application.SyncStatusLabel
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeEdge
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeExclusion
import org.harvestcircle.product.ScreenKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

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

    @Test
    fun collapsedMacOsSidebarRelocatesItsControlIntoSafeTopBarContent() =
        runComposeUiTest {
            var expandRequests = 0
            setHarvestCircleContent(
                windowChromeExclusion =
                    HarvestCircleWindowChromeExclusion(
                        edge = HarvestCircleWindowChromeEdge.Left,
                        width = 112.dp,
                        height = 40.dp,
                    ),
            ) {
                Box(Modifier.requiredSize(1280.dp, 720.dp)) {
                    DashboardScaffold(
                        inspectorVisible = false,
                        sidebarCollapsed = true,
                        topBar = { geometry ->
                            GlobalTopBar(
                                model =
                                    GlobalTopBarModel(
                                        canGoBack = false,
                                        canGoForward = false,
                                        syncStatus = SyncStatusLabel.Available,
                                        signerStatus = SignerStatusLabel.ReadOnly,
                                    ),
                                onIntent = {},
                                compact = true,
                                showSidebarToggle = geometry.sidebarTopBandFullyExcluded,
                                sidebarCollapsed = true,
                                onToggleSidebar = { expandRequests += 1 },
                            )
                        },
                        sidebar = { geometry ->
                            WorkspaceSidebar(
                                selected = ScreenKey.PersonalToday,
                                onScreen = {},
                                compact = true,
                                chromeClearance = geometry.sidebarChromeClearance,
                                topBandFullyExcluded = geometry.sidebarTopBandFullyExcluded,
                            )
                        },
                        mainHeader = {},
                        mainBody = {},
                    )
                }
            }

            val frame = onNodeWithTag("harvestcircle-frame").fetchSemanticsNode().boundsInRoot
            val safeTopBar = onNodeWithTag("harvestcircle-top-bar-chrome-content").fetchSemanticsNode().boundsInRoot
            val relocatedToggle = onNodeWithTag("top-bar-sidebar-toggle").fetchSemanticsNode().boundsInRoot

            assertEquals(frame.left + 112f, safeTopBar.left)
            assertTrue(relocatedToggle.left >= frame.left + 112f)
            onAllNodesWithTag("workspace-sidebar-toggle").assertCountEquals(0)
            onNodeWithTag("top-bar-sidebar-toggle").performClick()
            assertEquals(1, expandRequests)
        }
}

@androidx.compose.runtime.Composable
private fun dashboard(width: Int) {
    Box(Modifier.requiredSize(width.dp, 720.dp)) {
        DashboardScaffold(
            inspectorVisible = true,
            topBar = { _ -> },
            sidebar = { _ -> },
            mainHeader = {},
            mainBody = {},
            inspector = {},
        )
    }
}
