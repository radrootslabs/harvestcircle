package org.harvestcircle.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.designsystem.shell.HarvestCircleShellMetrics
import org.harvestcircle.designsystem.shell.HarvestCircleShellPalette
import org.harvestcircle.designsystem.shell.HarvestCircleShellTab
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole

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
    val colors = HarvestCircleShellPalette
    Row(
        modifier =
            Modifier
                .fillMaxSize()
                .background(colors.pane)
                .padding(horizontal = HarvestCircleShellMetrics.localHeaderHorizontalInset)
                .testTag("main-panel-header"),
        horizontalArrangement = Arrangement.spacedBy(HarvestCircleShellMetrics.localHeaderGap),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        HarvestCircleShellText(
            text = model.title,
            modifier =
                Modifier
                    .semantics { contentDescription = "Screen title: ${model.title}" }
                    .testTag("main-title"),
            role = HarvestCircleShellTextRole.PaneTitle,
            maxLines = 1,
        )
        if (model.breadcrumb.isNotEmpty()) {
            HarvestCircleShellText(
                text = model.breadcrumb.joinToString(" / "),
                modifier = Modifier.testTag("main-breadcrumb"),
                role = HarvestCircleShellTextRole.Small,
                color = colors.contentSecondary,
                maxLines = 1,
            )
        }
        model.tabs.forEach { tab ->
            HarvestCircleShellTab(
                label = tab.label,
                selected = tab.key == model.selectedTab,
                onClick = { if (tab.key != model.selectedTab) onTabSelected(tab.key) },
                modifier =
                    Modifier
                        .semantics { contentDescription = "Show ${tab.label}" }
                        .testTag("main-tab-${tab.key.value}"),
            )
        }
        Spacer(Modifier.weight(1f))
        model.localStatus?.let {
            HarvestCircleShellText(
                it,
                Modifier.testTag("main-local-status"),
                HarvestCircleShellTextRole.Small,
                colors.contentSecondary,
                maxLines = 1,
            )
        }
        Row(Modifier.testTag("main-secondary-action")) { secondaryAction() }
        Row(Modifier.testTag("main-primary-action")) { primaryAction() }
    }
}
