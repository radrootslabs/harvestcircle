package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.layout.HarvestCircleAppFrame
import org.harvestcircle.designsystem.layout.HarvestCirclePaneSlot
import org.harvestcircle.designsystem.layout.HarvestCirclePaneWidth
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import kotlin.math.roundToInt

@Composable
fun DashboardScaffold(
    inspectorVisible: Boolean,
    topBar: @Composable () -> Unit,
    sidebar: @Composable () -> Unit,
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
            sidebarCollapsed = false,
            sidebar = {
                DashboardRegion("dashboard-sidebar", "Workspace sidebar", Modifier.fillMaxSize(), sidebar)
            },
            topBar = {
                DashboardRegion("dashboard-top-bar", "Global top bar", Modifier.fillMaxSize(), topBar)
            },
            mainHeader = {
                DashboardRegion("dashboard-main-header", "Main panel header", Modifier.fillMaxSize(), mainHeader)
            },
            utilityPane = inspectorPane,
            mainContent = {
                DashboardRegion("dashboard-main-body", "Main panel body", Modifier.fillMaxSize(), mainBody)
            },
        )

        if (placement == InspectorPlacement.Overlay) {
            HarvestCircleSurface(
                modifier =
                    Modifier
                        .width(ShellDimensions.MINIMUM_INSPECTOR_WIDTH_DP.dp)
                        .fillMaxHeight()
                        .align(Alignment.CenterEnd),
                role = HarvestCircleSurfaceRole.Overlay,
            ) {
                DashboardRegion("dashboard-inspector-overlay", "Inspector", Modifier.fillMaxSize(), inspector)
            }
        }
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
