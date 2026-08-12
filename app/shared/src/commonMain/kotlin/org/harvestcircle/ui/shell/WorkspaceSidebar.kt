package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.ShellNavigationItem
import org.harvestcircle.application.addFarmWorkspaceAction
import org.harvestcircle.application.shellNavigationItems
import org.harvestcircle.application.shellSettingsItem
import org.harvestcircle.product.ScreenKey

@Composable
fun WorkspaceSidebar(
    selected: ScreenKey,
    onScreen: (ScreenKey) -> Unit,
) {
    Column(Modifier.fillMaxHeight().testTag("workspace-sidebar")) {
        ShellText("Workspace", Modifier.testTag("workspace-label"), ShellTextRole.SectionTitle)
        ShellText("Personal", Modifier.testTag("workspace-personal"), ShellTextRole.Secondary)
        ShellDivider()
        shellNavigationItems.forEach { item ->
            SidebarItem(item, selected == item.screenKey, onScreen)
        }
        DisabledWorkspaceAction()
        Spacer(Modifier.weight(1f))
        ShellDivider()
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
    ShellTab(
        label = item.label,
        description = description,
        selected = selected,
        onClick = { onScreen(item.screenKey) },
        modifier = Modifier.fillMaxWidth().testTag("sidebar-${item.screenKey.name}"),
        enabled = item.enabled,
    )
}

@Composable
private fun DisabledWorkspaceAction() {
    val action = addFarmWorkspaceAction
    ShellButton(
        label = action.label,
        description = "${action.label}. ${action.unavailableExplanation}",
        onClick = {},
        modifier = Modifier.fillMaxWidth().testTag("sidebar-add-farm"),
        enabled = false,
        kind = ShellButtonKind.Quiet,
    )
}
