package org.harvestcircle.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.absolutePadding
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.ShellNavigationItem
import org.harvestcircle.application.addFarmWorkspaceAction
import org.harvestcircle.application.shellNavigationItems
import org.harvestcircle.application.shellSettingsItem
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeClearance
import org.harvestcircle.designsystem.primitive.HarvestCircleIcon
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellIconButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMetrics
import org.harvestcircle.designsystem.shell.HarvestCircleShellNavigationItem
import org.harvestcircle.designsystem.shell.HarvestCircleShellPalette
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.product.ScreenKey
import org.jetbrains.compose.resources.DrawableResource

@Composable
fun WorkspaceSidebar(
    selected: ScreenKey,
    onScreen: (ScreenKey) -> Unit,
    compact: Boolean = false,
    onToggleCollapsed: () -> Unit = {},
    sessionLabel: String = "Read-only",
    chromeClearance: HarvestCircleWindowChromeClearance =
        HarvestCircleWindowChromeClearance(
            topBandHeight = HarvestCircleShellMetrics.sidebarHeaderHeight,
            left = 0.dp,
            right = 0.dp,
        ),
    topBandFullyExcluded: Boolean = false,
) {
    val colors = HarvestCircleShellPalette
    val chromeConsumesTitle =
        chromeClearance.left > HarvestCircleShellMetrics.sidebarHorizontalInset ||
            chromeClearance.right > HarvestCircleShellMetrics.sidebarHorizontalInset
    Column(
        Modifier
            .fillMaxHeight()
            .fillMaxWidth()
            .background(colors.sidebar)
            .testTag("workspace-sidebar"),
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .height(chromeClearance.topBandHeight)
                .absolutePadding(
                    left = maxOf(HarvestCircleShellMetrics.sidebarHorizontalInset, chromeClearance.left),
                    right = maxOf(10.dp, chromeClearance.right),
                ).testTag("workspace-sidebar-chrome-content"),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (!compact && !chromeConsumesTitle) {
                HarvestCircleShellText("HarvestCircle", Modifier.weight(1f), HarvestCircleShellTextRole.PaneTitle)
            } else {
                Spacer(Modifier.weight(1f))
            }
            if (!topBandFullyExcluded) {
                HarvestCircleShellIconButton(
                    onClick = onToggleCollapsed,
                    icon = HarvestCircleIcons.ChevronLeft,
                    label = if (compact) "Expand sidebar" else "Collapse sidebar",
                    modifier = Modifier.testTag("workspace-sidebar-toggle"),
                    controlSize = HarvestCircleShellMetrics.sidebarHeaderIconTarget,
                    iconSize = HarvestCircleShellMetrics.sidebarHeaderIconSize,
                )
            }
        }

        Box(Modifier.fillMaxWidth().padding(horizontal = HarvestCircleShellMetrics.sidebarHorizontalInset)) {
            HarvestCircleShellButton(
                text = if (compact) "+" else addFarmWorkspaceAction.label,
                onClick = {},
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .testTag("sidebar-add-farm")
                        .semantics {
                            contentDescription =
                                "${addFarmWorkspaceAction.label}. ${addFarmWorkspaceAction.unavailableExplanation}"
                        },
                enabled = false,
            )
        }
        Spacer(Modifier.height(HarvestCircleShellMetrics.sidebarQuickActionBottomGap))

        Column(
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = HarvestCircleShellMetrics.sidebarHorizontalInset),
        ) {
            if (!compact) SidebarSection("Personal")
            shellNavigationItems.forEach { item ->
                SidebarItem(item, selected == item.screenKey, compact, onScreen)
                Spacer(Modifier.height(HarvestCircleShellMetrics.sidebarNavigationGap))
            }
            Spacer(Modifier.weight(1f))
            SidebarItem(shellSettingsItem, selected == shellSettingsItem.screenKey, compact, onScreen)
            Spacer(Modifier.height(8.dp))
        }

        Box(Modifier.fillMaxWidth().height(HarvestCircleShellMetrics.structuralDividerWidth).background(colors.divider))
        Box(Modifier.fillMaxWidth().padding(HarvestCircleShellMetrics.sidebarFooterOuterInset)) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .height(HarvestCircleShellMetrics.sidebarFooterHeight)
                    .background(
                        colors.raised,
                        androidx.compose.foundation.shape
                            .RoundedCornerShape(HarvestCircleShellMetrics.controlRadius),
                    ).padding(horizontal = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                HarvestCircleIcon(
                    HarvestCircleIcons.Network,
                    null,
                    Modifier.size(HarvestCircleShellMetrics.sidebarFooterIconSize),
                    tint = colors.contentMuted,
                )
                if (!compact) {
                    Spacer(Modifier.width(8.dp))
                    HarvestCircleShellText(sessionLabel, Modifier.weight(1f), HarvestCircleShellTextRole.Label, maxLines = 1)
                }
            }
        }
    }
}

@Composable
private fun SidebarSection(label: String) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(HarvestCircleShellMetrics.sidebarSectionHeaderHeight)
            .padding(horizontal = HarvestCircleShellMetrics.sidebarNavigationHorizontalPadding),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        HarvestCircleShellText(label, role = HarvestCircleShellTextRole.Body, color = HarvestCircleShellPalette.contentSecondary)
    }
}

@Composable
private fun SidebarItem(
    item: ShellNavigationItem,
    selected: Boolean,
    compact: Boolean,
    onScreen: (ScreenKey) -> Unit,
) {
    HarvestCircleShellNavigationItem(
        label = item.label,
        icon = item.screenKey.icon(),
        selected = selected,
        onClick = { if (item.enabled && !selected) onScreen(item.screenKey) },
        modifier =
            Modifier
                .semantics {
                    contentDescription =
                        if (item.enabled) item.label else "${item.label}. ${item.unavailableExplanation}"
                }.testTag("sidebar-${item.screenKey.name}"),
        enabled = item.enabled,
        compact = compact,
    )
}

private fun ScreenKey.icon(): DrawableResource =
    when (this) {
        ScreenKey.PersonalToday -> HarvestCircleIcons.Today
        ScreenKey.Network -> HarvestCircleIcons.Network
        ScreenKey.Settings -> HarvestCircleIcons.Settings
        else -> HarvestCircleIcons.Activity
    }
