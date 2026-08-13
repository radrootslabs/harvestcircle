package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.application.SignerStatusLabel
import org.harvestcircle.application.SyncStatusLabel
import org.harvestcircle.designsystem.component.HarvestCircleButtonVariant
import org.harvestcircle.designsystem.component.HarvestCircleControlSize
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.action.HarvestCircleButton
import org.harvestcircle.designsystem.component.action.HarvestCircleIconButton
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.component.menu.HarvestCirclePopupButton
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import org.harvestcircle.designsystem.layout.HarvestCircleToolbar
import org.harvestcircle.designsystem.primitive.HarvestCircleText

data class GlobalTopBarModel(
    val canGoBack: Boolean,
    val canGoForward: Boolean,
    val syncStatus: SyncStatusLabel,
    val signerStatus: SignerStatusLabel,
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
) {
    HarvestCircleToolbar(Modifier.fillMaxSize().testTag("global-top-bar")) {
        HarvestCircleIconButton(
            onClick = { onIntent(GlobalTopBarIntent.Back) },
            icon = HarvestCircleIcons.ChevronLeft,
            label = "Go back",
            modifier = Modifier.testTag("top-bar-back"),
            size = HarvestCircleControlSize.Small,
            enabled = model.canGoBack,
        )
        HarvestCircleIconButton(
            onClick = { onIntent(GlobalTopBarIntent.Forward) },
            icon = HarvestCircleIcons.ChevronRight,
            label = "Go forward",
            modifier = Modifier.testTag("top-bar-forward"),
            size = HarvestCircleControlSize.Small,
            enabled = model.canGoForward,
        )
        HarvestCircleButton(
            onClick = { onIntent(GlobalTopBarIntent.OpenNostrReference) },
            modifier =
                Modifier
                    .shellFocusTarget(ShellFocusTarget.TopBarReference)
                    .semantics { contentDescription = "Open a Nostr reference" }
                    .testTag("top-bar-open-reference"),
            variant = HarvestCircleButtonVariant.Ghost,
            size = HarvestCircleControlSize.Small,
            leadingIcon = {
                org.harvestcircle.designsystem.primitive.HarvestCircleIcon(
                    resource = HarvestCircleIcons.Search,
                    contentDescription = null,
                )
            },
        ) {
            HarvestCircleText("Open a Nostr reference", role = HarvestCircleTextRole.Label)
        }

        Spacer(Modifier.weight(1f))
        StatusAction(model.syncStatus.text, "Sync status", "top-bar-sync") {
            onIntent(GlobalTopBarIntent.ShowSyncStatus)
        }
        StatusAction(model.signerStatus.text, "Signer status", "top-bar-signer") {
            onIntent(GlobalTopBarIntent.ShowSignerStatus)
        }
        HarvestCirclePopupButton(
            selectedValue = ApplicationMenuAction.Settings,
            options = ApplicationMenuAction.entries.map { HarvestCircleMenuOption(it, it.label) },
            onValueChange = { onIntent(GlobalTopBarIntent.SelectApplicationMenu(it)) },
            modifier = Modifier.testTag("top-bar-menu"),
            buttonLabel = "Menu",
            showSelection = false,
            size = HarvestCircleControlSize.Small,
        )
    }
}

@Composable
private fun StatusAction(
    label: String,
    description: String,
    tag: String,
    onClick: () -> Unit,
) {
    HarvestCircleButton(
        onClick = onClick,
        modifier =
            Modifier
                .shellFocusTarget(
                    if (tag == "top-bar-sync") ShellFocusTarget.TopBarSync else ShellFocusTarget.TopBarSigner,
                ).semantics { contentDescription = description }
                .testTag(tag),
        variant = HarvestCircleButtonVariant.Ghost,
        size = HarvestCircleControlSize.Small,
    ) {
        HarvestCircleText(label, role = HarvestCircleTextRole.Label)
    }
}
