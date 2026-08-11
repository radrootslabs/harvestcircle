package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp

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

@Composable
fun SingleFocusTemplate(content: @Composable () -> Unit) {
    Box(Modifier.fillMaxSize().testTag("template-single-focus")) { content() }
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
    detail: @Composable (TemplateSelectionKey) -> Unit,
) {
    require(tabs.map(TemplateTab::key).distinct().size == tabs.size)
    require(tabs.any { it.key == selected })
    Column(Modifier.fillMaxSize().testTag("template-tabbed-detail")) {
        Box(Modifier.testTag("template-tabs")) { tabRail(tabs, selected) }
        Box(Modifier.weight(1f).testTag("template-tab-detail")) { detail(selected) }
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
