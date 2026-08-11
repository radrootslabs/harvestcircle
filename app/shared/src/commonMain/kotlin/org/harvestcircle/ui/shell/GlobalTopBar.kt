package org.harvestcircle.ui.shell

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.harvestcircle.design.HarvestCircleDesign

enum class SyncStatusLabel(
    val text: String,
) {
    NotYetObserved("Not yet observed"),
    Available("Available"),
    Degraded("Degraded"),
    Unavailable("Unavailable"),
}

enum class SignerStatusLabel(
    val text: String,
) {
    ReadOnly("Read-only"),
    SignedOut("No signer"),
    Available("Signer available"),
    CredentialMissing("Credential missing"),
}

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
        ShellAction("Open reference", "Open Nostr reference", "top-bar-open-reference") {
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
    var focused by remember { mutableStateOf(false) }
    val focusColor = LocalHarvestCirclePalette.current.focus.toComposeColor()
    BasicText(
        text = label,
        modifier =
            Modifier
                .heightIn(min = HarvestCircleDesign.MINIMUM_TARGET_DP.dp)
                .onFocusChanged { focused = it.isFocused }
                .border(
                    width = HarvestCircleDesign.BORDER_DP.dp,
                    color = if (focused) focusColor else androidx.compose.ui.graphics.Color.Transparent,
                ).padding(horizontal = HarvestCircleDesign.spacingDp[2].dp)
                .semantics {
                    contentDescription = description
                    role = Role.Button
                    if (!enabled) disabled()
                }.then(if (enabled) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier)
                .testTag(tag),
    )
}
