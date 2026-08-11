package org.harvestcircle.ui.shell

import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.OverlayState

@Composable
fun FoundationOverlayHost(
    state: OverlayState,
    busy: Boolean = false,
    onIntent: (OverlayIntent) -> Unit,
) {
    state.banner?.let { banner ->
        ShellBadge(
            banner.message,
            Modifier.semantics { contentDescription = "Status: ${banner.message}" }.testTag("global-status-banner"),
        )
    }
    state.current?.let { overlay ->
        Dialog(onDismissRequest = { if (!busy) onIntent(OverlayIntent.Close) }) {
            ShellSurface(
                Modifier
                    .focusGroup()
                    .semantics { contentDescription = "Dialog" }
                    .testTag("foundation-overlay")
                    .padding(24.dp),
            ) {
                when (overlay) {
                    is FoundationOverlay.ConfirmAction -> ConfirmOverlay(overlay, busy, onIntent)
                    is FoundationOverlay.SignerStatus -> StatusOverlay("Signer status", overlay.status.text, onIntent)
                    is FoundationOverlay.SyncStatus -> StatusOverlay("Sync status", overlay.status.text, onIntent)
                    is FoundationOverlay.OpenNostrReference -> ReferenceOverlay(overlay, busy, onIntent)
                }
            }
        }
    }
}

@Composable
private fun ConfirmOverlay(
    overlay: FoundationOverlay.ConfirmAction,
    busy: Boolean,
    onIntent: (OverlayIntent) -> Unit,
) {
    val requester = remember { FocusRequester() }
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        ShellText(overlay.title, textRole = ShellTextRole.SectionTitle)
        ShellText(overlay.explanation)
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            ShellButton(
                overlay.actionLabel,
                overlay.actionLabel,
                { onIntent(OverlayIntent.Confirm) },
                Modifier.focusRequester(requester).testTag("overlay-confirm"),
                enabled = !busy,
                kind = ShellButtonKind.Destructive,
            )
            ShellButton("Cancel", "Cancel", { onIntent(OverlayIntent.Close) }, Modifier.testTag("overlay-cancel"), !busy)
        }
    }
    LaunchedEffect(Unit) { requester.requestFocus() }
}

@Composable
private fun StatusOverlay(
    title: String,
    status: String,
    onIntent: (OverlayIntent) -> Unit,
) {
    val requester = remember { FocusRequester() }
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        ShellText(title, textRole = ShellTextRole.SectionTitle)
        ShellText(status, Modifier.testTag("overlay-status"))
        ShellButton(
            "Close",
            "Close",
            { onIntent(OverlayIntent.Close) },
            Modifier.focusRequester(requester).testTag("overlay-close"),
        )
    }
    LaunchedEffect(Unit) { requester.requestFocus() }
}

@Composable
private fun ReferenceOverlay(
    overlay: FoundationOverlay.OpenNostrReference,
    busy: Boolean,
    onIntent: (OverlayIntent) -> Unit,
) {
    val requester = remember { FocusRequester() }
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        ShellText("Open a Nostr reference", textRole = ShellTextRole.SectionTitle)
        ShellTextField(
            value = overlay.input,
            onValueChange = { onIntent(OverlayIntent.EditReference(it)) },
            label = "Nostr reference",
            placeholder = "npub1…, note1…, or nevent1…",
            modifier = Modifier.focusRequester(requester).testTag("nostr-reference-input"),
            enabled = !busy,
        )
        overlay.result?.let { ShellText(it.message, Modifier.testTag("nostr-reference-result")) }
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            ShellButton(
                "Open reference",
                "Open Nostr reference",
                { onIntent(OverlayIntent.SubmitReference) },
                Modifier.testTag("nostr-reference-submit"),
                enabled = !busy,
                kind = ShellButtonKind.Primary,
            )
            ShellButton("Cancel", "Cancel", { onIntent(OverlayIntent.Close) }, Modifier.testTag("overlay-cancel"), !busy)
        }
    }
    LaunchedEffect(Unit) { requester.requestFocus() }
}
