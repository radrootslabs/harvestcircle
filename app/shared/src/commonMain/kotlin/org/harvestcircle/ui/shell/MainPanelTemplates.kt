package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.layout.HarvestCirclePane
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@JvmInline
value class TemplateSelectionKey(
    val value: String,
) {
    init {
        require(value.matches(Regex("[a-z][a-z0-9_-]{0,63}")))
    }
}

data class TemplateTab(
    val key: TemplateSelectionKey,
    val label: String,
) {
    init {
        require(label.isNotBlank())
    }
}

enum class DetailPaneKind { Network, Settings }

@Composable
fun SingleFocusTemplate(content: @Composable () -> Unit) {
    HarvestCirclePane(
        modifier = Modifier.fillMaxSize().testTag("template-single-focus"),
        role = HarvestCircleSurfaceRole.Canvas,
    ) {
        content()
    }
}

@Composable
fun MasterDetailTemplate(
    selection: TemplateSelectionKey?,
    master: @Composable () -> Unit,
    detail: @Composable (TemplateSelectionKey?) -> Unit,
) {
    Row(Modifier.fillMaxSize().testTag("template-master-detail")) {
        Column(
            Modifier
                .width(320.dp)
                .fillMaxHeight()
                .verticalScroll(rememberScrollState())
                .testTag("template-master-list"),
        ) {
            master()
        }
        Box(Modifier.weight(1f).fillMaxHeight().testTag("template-detail")) { detail(selection) }
    }
}

@Composable
fun TabbedDetailTemplate(
    tabs: List<TemplateTab>,
    selected: TemplateSelectionKey,
    tabRail: @Composable (List<TemplateTab>, TemplateSelectionKey) -> Unit,
    detailPane: DetailPaneKind? = null,
    detail: @Composable (TemplateSelectionKey) -> Unit,
) {
    require(tabs.map(TemplateTab::key).distinct().size == tabs.size)
    require(tabs.any { it.key == selected })
    Column(
        Modifier
            .fillMaxSize()
            .padding(HarvestCircleTheme.shell.layout.paneInset)
            .testTag("template-tabbed-detail"),
    ) {
        Box(
            Modifier
                .padding(bottom = HarvestCircleTheme.shell.layout.contentGap)
                .testTag("template-tabs"),
        ) {
            tabRail(tabs, selected)
        }
        val detailModifier =
            Modifier
                .weight(1f)
                .then(
                    if (detailPane != null) {
                        Modifier.verticalScroll(rememberScrollState()).testTag("bounded-detail-${detailPane.name.lowercase()}")
                    } else {
                        Modifier
                    },
                ).testTag("template-tab-detail")
        Box(detailModifier) { detail(selected) }
    }
}

@Composable
fun StudioTemplate(
    rail: @Composable () -> Unit,
    body: @Composable () -> Unit,
    action: @Composable () -> Unit,
) {
    Row(Modifier.fillMaxSize().testTag("template-workbench")) {
        Box(Modifier.width(280.dp).fillMaxHeight().testTag("template-workbench-rail")) { rail() }
        Column(Modifier.weight(1f).fillMaxHeight()) {
            Box(Modifier.weight(1f).testTag("template-workbench-body")) { body() }
            Box(Modifier.testTag("template-workbench-action")) { action() }
        }
    }
}
