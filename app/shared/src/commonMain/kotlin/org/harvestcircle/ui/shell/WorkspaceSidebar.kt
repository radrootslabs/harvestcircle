package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.application.ShellNavigationItem
import org.harvestcircle.application.addFarmWorkspaceAction
import org.harvestcircle.application.shellNavigationItems
import org.harvestcircle.application.shellSettingsItem
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.navigation.HarvestCircleNavigationItem
import org.harvestcircle.designsystem.component.utility.HarvestCircleHorizontalDivider
import org.harvestcircle.designsystem.layout.HarvestCircleSidebar
import org.harvestcircle.designsystem.layout.HarvestCircleSidebarSectionHeader
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.harvestcircle.product.ScreenKey

@Composable
fun WorkspaceSidebar(
    selected: ScreenKey,
    onScreen: (ScreenKey) -> Unit,
) {
    HarvestCircleSidebar(Modifier.fillMaxHeight().testTag("workspace-sidebar")) {
        HarvestCircleSidebarSectionHeader("Navigation", Modifier.testTag("workspace-label"))
        shellNavigationItems.forEach { item ->
            SidebarItem(item, selected == item.screenKey, onScreen)
        }

        HarvestCircleSidebarSectionHeader("Workspaces")
        DisabledWorkspaceAction()
        Spacer(Modifier.weight(1f))
        HarvestCircleHorizontalDivider()
        SidebarItem(shellSettingsItem, selected == shellSettingsItem.screenKey, onScreen)
    }
}

@Composable
private fun SidebarItem(
    item: ShellNavigationItem,
    selected: Boolean,
    onScreen: (ScreenKey) -> Unit,
) {
    val description =
        if (item.enabled) {
            item.label
        } else {
            "${item.label}. ${item.unavailableExplanation}"
        }
    Column {
        HarvestCircleNavigationItem(
            selected = selected,
            onClick = { if (item.enabled && !selected) onScreen(item.screenKey) },
            label = item.label,
            modifier =
                Modifier
                    .fillMaxWidth()
                    .semantics { contentDescription = description }
                    .testTag("sidebar-${item.screenKey.name}"),
            enabled = item.enabled,
        )
        if (!item.enabled) {
            HarvestCircleText(
                text = requireNotNull(item.unavailableExplanation),
                modifier = Modifier.padding(horizontal = HarvestCircleTheme.foundation.spacing.md),
                role = HarvestCircleTextRole.LabelSmall,
                tone = HarvestCircleContentTone.Muted,
            )
        }
    }
}

@Composable
private fun DisabledWorkspaceAction() {
    val action = addFarmWorkspaceAction
    Column {
        HarvestCircleNavigationItem(
            selected = false,
            onClick = {},
            label = action.label,
            modifier =
                Modifier
                    .fillMaxWidth()
                    .semantics { contentDescription = "${action.label}. ${action.unavailableExplanation}" }
                    .testTag("sidebar-add-farm"),
            enabled = false,
        )
        HarvestCircleText(
            text = requireNotNull(action.unavailableExplanation),
            modifier = Modifier.padding(horizontal = HarvestCircleTheme.foundation.spacing.md),
            role = HarvestCircleTextRole.LabelSmall,
            tone = HarvestCircleContentTone.Muted,
        )
    }
}
