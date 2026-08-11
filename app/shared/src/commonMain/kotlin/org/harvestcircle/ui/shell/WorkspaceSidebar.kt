package org.harvestcircle.ui.shell

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.ShellDestination
import org.harvestcircle.application.ShellNavigationItem
import org.harvestcircle.application.shellNavigationItems

@Composable
fun WorkspaceSidebar(
    selected: ShellDestination,
    onDestination: (ShellDestination) -> Unit,
) {
    Column(Modifier.testTag("workspace-sidebar")) {
        BasicText("Workspace", Modifier.testTag("workspace-label"))
        BasicText("Personal", Modifier.testTag("workspace-personal"))
        shellNavigationItems.forEach { item ->
            SidebarItem(item, selected == item.destination, onDestination)
        }
    }
}

@Composable
private fun SidebarItem(
    item: ShellNavigationItem,
    selected: Boolean,
    onDestination: (ShellDestination) -> Unit,
) {
    val description =
        if (item.enabled) {
            item.label
        } else {
            "${item.label}. ${item.unavailableExplanation}"
        }
    BasicText(
        text = item.label,
        modifier =
            Modifier
                .fillMaxWidth()
                .heightIn(min = 44.dp)
                .semantics {
                    contentDescription = description
                    role = Role.Tab
                    this.selected = selected
                    if (!item.enabled) disabled()
                }.then(
                    if (item.enabled) {
                        Modifier.clickable(role = Role.Tab) { onDestination(item.destination) }
                    } else {
                        Modifier
                    },
                ).padding(horizontal = 16.dp, vertical = 12.dp)
                .testTag("sidebar-${item.destination.name}"),
    )
}
