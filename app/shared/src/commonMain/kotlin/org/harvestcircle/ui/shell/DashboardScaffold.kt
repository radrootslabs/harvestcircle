package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

@Composable
fun DashboardScaffold(
    windowWidthDp: Int,
    inspectorVisible: Boolean,
    topBar: @Composable () -> Unit,
    sidebar: @Composable () -> Unit,
    mainHeader: @Composable () -> Unit,
    mainBody: @Composable () -> Unit,
    inspector: @Composable () -> Unit = {},
) {
    val placement = inspectorPlacement(windowWidthDp, inspectorVisible)
    Column(Modifier.fillMaxSize().testTag("dashboard-scaffold")) {
        Region("dashboard-top-bar", "Global top bar", Modifier.fillMaxWidth().height(56.dp), topBar)
        Row(Modifier.fillMaxSize()) {
            Region("dashboard-sidebar", "Workspace sidebar", Modifier.width(232.dp).fillMaxHeight(), sidebar)
            Column(Modifier.weight(1f).fillMaxHeight()) {
                Region("dashboard-main-header", "Main panel header", Modifier.fillMaxWidth().height(56.dp), mainHeader)
                Region("dashboard-main-body", "Main panel body", Modifier.fillMaxSize(), mainBody)
            }
            if (placement == InspectorPlacement.Beside) {
                Region("dashboard-inspector-beside", "Inspector", Modifier.width(400.dp).fillMaxHeight(), inspector)
            }
        }
    }
    if (placement == InspectorPlacement.Overlay) {
        Box(Modifier.fillMaxSize(), contentAlignment = androidx.compose.ui.Alignment.CenterEnd) {
            Region("dashboard-inspector-overlay", "Inspector", Modifier.width(360.dp).fillMaxHeight(), inspector)
        }
    }
}

@Composable
private fun Region(
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
