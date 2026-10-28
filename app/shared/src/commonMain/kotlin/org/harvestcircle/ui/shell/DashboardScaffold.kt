package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.designsystem.layout.HarvestCircleAppFrame
import org.harvestcircle.designsystem.layout.HarvestCircleFrameGeometry
import org.harvestcircle.designsystem.layout.HarvestCirclePaneSlot
import org.harvestcircle.designsystem.layout.HarvestCirclePaneWidth
import kotlin.math.roundToInt

@Composable
fun DashboardScaffold(
    inspectorVisible: Boolean,
    sidebarCollapsed: Boolean = false,
    topBar: @Composable (HarvestCircleFrameGeometry) -> Unit,
    sidebar: @Composable (HarvestCircleFrameGeometry) -> Unit,
    mainHeader: @Composable () -> Unit,
    mainBody: @Composable () -> Unit,
    inspector: @Composable () -> Unit = {},
) {
    BoxWithConstraints(Modifier.fillMaxSize().testTag("dashboard-scaffold")) {
        val placement = inspectorPlacement(maxWidth.value.roundToInt(), inspectorVisible)
        val inspectorPane =
            if (placement == InspectorPlacement.Beside) {
                HarvestCirclePaneSlot(
                    width = HarvestCirclePaneWidth.Inspector,
                    header = {},
                    content = {
                        DashboardRegion("dashboard-inspector-beside", "Inspector", Modifier.fillMaxSize(), inspector)
                    },
                )
            } else {
                null
            }

        HarvestCircleAppFrame(
            sidebarCollapsed = sidebarCollapsed,
            sidebar = { geometry ->
                DashboardRegion("dashboard-sidebar", "Workspace sidebar", Modifier.fillMaxSize()) {
                    sidebar(geometry)
                }
            },
            topBar = { geometry ->
                DashboardRegion("dashboard-top-bar", "Global top bar", Modifier.fillMaxSize()) {
                    topBar(geometry)
                }
            },
            mainHeader = {
                DashboardRegion("dashboard-main-header", "Main panel header", Modifier.fillMaxSize(), mainHeader)
            },
            utilityPane = inspectorPane,
            mainContent = {
                DashboardRegion("dashboard-main-body", "Main panel body", Modifier.fillMaxSize()) {
                    if (placement == InspectorPlacement.FullRecord) {
                        DashboardRegion("dashboard-inspector-record", "Full record", Modifier.fillMaxSize(), inspector)
                    } else {
                        mainBody()
                    }
                }
            },
        )
    }
}

@Composable
private fun DashboardRegion(
    tag: String,
    label: String,
    modifier: Modifier,
    content: @Composable () -> Unit,
) {
    Box(
        modifier =
            modifier
                .semantics { contentDescription = label }
                .testTag(tag),
    ) {
        content()
    }
}
