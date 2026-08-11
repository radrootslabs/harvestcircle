package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
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
            ShellText(
                model.title,
                Modifier.semantics { contentDescription = "Screen title: ${model.title}" }.testTag("main-title"),
                ShellTextRole.ScreenTitle,
            )
            if (model.breadcrumb.isNotEmpty()) {
                ShellText(model.breadcrumb.joinToString(" / "), Modifier.testTag("main-breadcrumb"), ShellTextRole.Secondary)
            }
            model.localStatus?.let { ShellBadge(it, Modifier.testTag("main-local-status")) }
        }
        model.tabs.forEach { tab ->
            ShellTab(
                label = tab.label,
                description = "Show ${tab.label}",
                selected = tab.key == model.selectedTab,
                onClick = { onTabSelected(tab.key) },
                modifier = Modifier.testTag("main-tab-${tab.key.value}"),
                enabled = tab.key != model.selectedTab,
            )
        }
        Row(Modifier.testTag("main-secondary-action")) { secondaryAction() }
        Row(Modifier.testTag("main-primary-action")) { primaryAction() }
    }
}
