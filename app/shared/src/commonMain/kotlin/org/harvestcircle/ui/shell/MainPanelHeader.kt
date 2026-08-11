package org.harvestcircle.ui.shell

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

data class MainPanelHeaderModel(
    val title: String,
    val breadcrumb: List<String> = emptyList(),
    val localStatus: String? = null,
    val tabs: List<TemplateTab> = emptyList(),
    val selectedTab: TemplateSelectionKey? = null,
) {
    init {
        require(title.isNotBlank())
        require(breadcrumb.all(String::isNotBlank))
        require(tabs.isEmpty() == (selectedTab == null))
        require(selectedTab == null || tabs.any { it.key == selectedTab })
    }
}

@Composable
fun MainPanelHeader(
    model: MainPanelHeaderModel,
    onTabSelected: (TemplateSelectionKey) -> Unit = {},
    secondaryAction: @Composable () -> Unit = {},
    primaryAction: @Composable () -> Unit = {},
) {
    Row(
        Modifier.fillMaxSize().testTag("main-panel-header"),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Column(Modifier.weight(1f)) {
            BasicText(model.title, Modifier.semantics { contentDescription = "Screen title: ${model.title}" }.testTag("main-title"))
            if (model.breadcrumb.isNotEmpty()) {
                BasicText(model.breadcrumb.joinToString(" / "), Modifier.testTag("main-breadcrumb"))
            }
            model.localStatus?.let { BasicText(it, Modifier.testTag("main-local-status")) }
        }
        model.tabs.forEach { tab ->
            BasicText(
                tab.label,
                Modifier
                    .semantics {
                        role = Role.Tab
                        selected = tab.key == model.selectedTab
                    }.clickable(role = Role.Tab) { onTabSelected(tab.key) }
                    .testTag("main-tab-${tab.key.value}"),
            )
        }
        Row(Modifier.testTag("main-secondary-action")) { secondaryAction() }
        Row(Modifier.testTag("main-primary-action")) { primaryAction() }
    }
}
