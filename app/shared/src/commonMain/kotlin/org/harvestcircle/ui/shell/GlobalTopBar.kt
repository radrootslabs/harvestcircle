package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.SignerStatusLabel
import org.harvestcircle.application.SyncStatusLabel

data class GlobalTopBarModel(
    val canGoBack: Boolean,
    val canGoForward: Boolean,
    val syncStatus: SyncStatusLabel,
    val signerStatus: SignerStatusLabel,
)

sealed interface GlobalTopBarIntent {
    data object Back : GlobalTopBarIntent

    data object Forward : GlobalTopBarIntent

    data object OpenNostrReference : GlobalTopBarIntent

    data object ShowSyncStatus : GlobalTopBarIntent

    data object ShowSignerStatus : GlobalTopBarIntent

    data object OpenApplicationMenu : GlobalTopBarIntent
}

@Composable
fun GlobalTopBar(
    model: GlobalTopBarModel,
    onIntent: (GlobalTopBarIntent) -> Unit,
) {
    Row(
        Modifier.fillMaxSize().testTag("global-top-bar"),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        ShellAction("Back", "Go back", "top-bar-back", model.canGoBack) { onIntent(GlobalTopBarIntent.Back) }
        ShellAction("Forward", "Go forward", "top-bar-forward", model.canGoForward) {
            onIntent(GlobalTopBarIntent.Forward)
        }
        ShellAction("Open a Nostr reference", "Open a Nostr reference", "top-bar-open-reference") {
            onIntent(GlobalTopBarIntent.OpenNostrReference)
        }
        ShellAction(model.syncStatus.text, "Sync status", "top-bar-sync") { onIntent(GlobalTopBarIntent.ShowSyncStatus) }
        ShellAction(model.signerStatus.text, "Signer status", "top-bar-signer") {
            onIntent(GlobalTopBarIntent.ShowSignerStatus)
        }
        ShellAction("Menu", "Application menu", "top-bar-menu") { onIntent(GlobalTopBarIntent.OpenApplicationMenu) }
    }
}

@Composable
internal fun ShellAction(
    label: String,
    description: String,
    tag: String,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    ShellButton(label, description, onClick, Modifier.testTag(tag), enabled)
}
