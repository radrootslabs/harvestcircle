package org.harvestcircle.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.application.SignerStatusLabel
import org.harvestcircle.application.SyncStatusLabel
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import org.harvestcircle.designsystem.primitive.HarvestCircleIcon
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellIconButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMenuButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMetrics
import org.harvestcircle.designsystem.shell.HarvestCircleShellPalette
import org.harvestcircle.designsystem.shell.HarvestCircleShellTab
import org.harvestcircle.product.ScreenKey

data class GlobalTopBarModel(
    val canGoBack: Boolean,
    val canGoForward: Boolean,
    val syncStatus: SyncStatusLabel,
    val signerStatus: SignerStatusLabel,
    val selectedScreen: ScreenKey = ScreenKey.PersonalToday,
)

enum class ApplicationMenuAction(
    val label: String,
) {
    Settings("Settings"),
    AboutBuild("About this build"),
    Source("Source"),
    Licence("Licence"),
}

sealed interface GlobalTopBarIntent {
    data object Back : GlobalTopBarIntent

    data object Forward : GlobalTopBarIntent

    data object OpenToday : GlobalTopBarIntent

    data object OpenNetwork : GlobalTopBarIntent

    data object OpenNostrReference : GlobalTopBarIntent

    data object ShowSyncStatus : GlobalTopBarIntent

    data object ShowSignerStatus : GlobalTopBarIntent

    data class SelectApplicationMenu(
        val action: ApplicationMenuAction,
    ) : GlobalTopBarIntent
}

@Composable
fun GlobalTopBar(
    model: GlobalTopBarModel,
    onIntent: (GlobalTopBarIntent) -> Unit,
    compact: Boolean = false,
    showSidebarToggle: Boolean = false,
    sidebarCollapsed: Boolean = false,
    onToggleSidebar: () -> Unit = {},
) {
    val colors = HarvestCircleShellPalette
    Row(
        Modifier
            .fillMaxWidth()
            .fillMaxHeight()
            .background(colors.applicationFrame)
            .padding(horizontal = HarvestCircleShellMetrics.topBarHorizontalInset)
            .testTag("global-top-bar"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(HarvestCircleShellMetrics.topBarControlGap),
    ) {
        if (showSidebarToggle) {
            HarvestCircleShellIconButton(
                onClick = onToggleSidebar,
                icon = if (sidebarCollapsed) HarvestCircleIcons.ChevronRight else HarvestCircleIcons.ChevronLeft,
                label = if (sidebarCollapsed) "Expand sidebar" else "Collapse sidebar",
                modifier = Modifier.testTag("top-bar-sidebar-toggle"),
            )
        }
        HarvestCircleShellTab(
            "Today",
            model.selectedScreen == ScreenKey.PersonalToday,
            { onIntent(GlobalTopBarIntent.OpenToday) },
            icon = HarvestCircleIcons.Today,
        )
        HarvestCircleShellTab(
            "Network",
            model.selectedScreen == ScreenKey.Network,
            { onIntent(GlobalTopBarIntent.OpenNetwork) },
            icon = HarvestCircleIcons.Network,
        )
        HarvestCircleShellIconButton(
            onClick = { onIntent(GlobalTopBarIntent.Back) },
            icon = HarvestCircleIcons.ChevronLeft,
            label = "Go back",
            modifier = Modifier.testTag("top-bar-back"),
            enabled = model.canGoBack,
        )
        HarvestCircleShellIconButton(
            onClick = { onIntent(GlobalTopBarIntent.Forward) },
            icon = HarvestCircleIcons.ChevronRight,
            label = "Go forward",
            modifier = Modifier.testTag("top-bar-forward"),
            enabled = model.canGoForward,
        )
        Spacer(Modifier.weight(1f))
        if (compact) {
            HarvestCircleShellIconButton(
                onClick = { onIntent(GlobalTopBarIntent.OpenNostrReference) },
                icon = HarvestCircleIcons.Search,
                label = "Open a Nostr reference",
                modifier =
                    Modifier
                        .shellFocusTarget(ShellFocusTarget.TopBarReference)
                        .testTag("top-bar-open-reference"),
            )
        } else {
            HarvestCircleShellButton(
                text = "Open a Nostr reference",
                onClick = { onIntent(GlobalTopBarIntent.OpenNostrReference) },
                modifier =
                    Modifier
                        .shellFocusTarget(ShellFocusTarget.TopBarReference)
                        .testTag("top-bar-open-reference"),
                leadingContent = {
                    HarvestCircleIcon(
                        HarvestCircleIcons.Search,
                        null,
                        tint = colors.contentSecondary,
                    )
                },
            )
        }
        HarvestCircleShellButton(
            model.syncStatus.text,
            { onIntent(GlobalTopBarIntent.ShowSyncStatus) },
            Modifier.shellFocusTarget(ShellFocusTarget.TopBarSync).testTag("top-bar-sync"),
        )
        HarvestCircleShellButton(
            model.signerStatus.text,
            { onIntent(GlobalTopBarIntent.ShowSignerStatus) },
            Modifier.shellFocusTarget(ShellFocusTarget.TopBarSigner).testTag("top-bar-signer"),
        )
        HarvestCircleShellMenuButton(
            selectedValue = ApplicationMenuAction.Settings,
            options = ApplicationMenuAction.entries.map { HarvestCircleMenuOption(it, it.label) },
            onValueChange = { onIntent(GlobalTopBarIntent.SelectApplicationMenu(it)) },
            modifier = Modifier.testTag("top-bar-menu"),
            label = "Menu",
        )
    }
}
