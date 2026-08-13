package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
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
