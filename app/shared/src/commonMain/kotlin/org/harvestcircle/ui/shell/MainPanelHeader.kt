package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBadge
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTab
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTabRow
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

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
        modifier =
            Modifier
                .fillMaxSize()
                .padding(horizontal = HarvestCircleTheme.shell.layout.paneInset)
                .testTag("main-panel-header"),
        horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.lg),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            HarvestCircleText(
                text = model.title,
                modifier =
                    Modifier
                        .semantics { contentDescription = "Screen title: ${model.title}" }
                        .testTag("main-title"),
                role = HarvestCircleTextRole.PageTitle,
            )
            if (model.breadcrumb.isNotEmpty()) {
                HarvestCircleText(
                    text = model.breadcrumb.joinToString(" / "),
                    modifier = Modifier.testTag("main-breadcrumb"),
                    role = HarvestCircleTextRole.LabelSmall,
                    tone = HarvestCircleContentTone.Secondary,
                )
            }
        }
        model.localStatus?.let { HarvestCircleBadge(it, Modifier.testTag("main-local-status")) }
        if (model.tabs.isNotEmpty()) {
            HarvestCircleTabRow {
                model.tabs.forEach { tab ->
                    HarvestCircleTab(
                        selected = tab.key == model.selectedTab,
                        onClick = { if (tab.key != model.selectedTab) onTabSelected(tab.key) },
                        label = tab.label,
                        modifier =
                            Modifier
                                .semantics { contentDescription = "Show ${tab.label}" }
                                .testTag("main-tab-${tab.key.value}"),
                    )
                }
            }
        }
        Row(Modifier.testTag("main-secondary-action")) { secondaryAction() }
        Row(Modifier.testTag("main-primary-action")) { primaryAction() }
    }
}
