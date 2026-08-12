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
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.OverlayState
import org.harvestcircle.application.ShellStatusModel
import org.harvestcircle.application.StatusOverlayKey

@Composable
fun FoundationOverlayHost(
    state: OverlayState,
    status: ShellStatusModel,
    busy: Boolean = false,
    onIntent: (OverlayIntent) -> Unit,
) {
    status.banner?.let { banner ->
        ShellCard(
            Modifier.semantics { contentDescription = "Status: ${banner.title}. ${banner.message}" }.testTag("global-status-banner"),
        ) {
            Column {
                ShellText(banner.title, textRole = ShellTextRole.CardTitle)
                ShellText(banner.message)
            }
        }
    }
    state.current?.let { overlay ->
        val overlayBusy = busy || (overlay as? FoundationOverlay.ConfirmAction)?.busy == true
        Dialog(
            onDismissRequest = {
                if (!overlayBusy) {
                    if (overlay is FoundationOverlay.ConfirmAction) {
                        onIntent(OverlayIntent.DismissConfirmation(overlay.action))
                    } else {
                        onIntent(OverlayIntent.Close)
                    }
                }
            },
        ) {
            ShellSurface(
                Modifier
                    .focusGroup()
                    .semantics {
                        contentDescription = "Dialog: ${overlay.title()}"
                        paneTitle = overlay.title()
                    }.testTag("foundation-overlay")
                    .padding(24.dp),
            ) {
                when (overlay) {
                    is FoundationOverlay.ConfirmAction -> ConfirmOverlay(overlay, overlayBusy, onIntent)
                    is FoundationOverlay.Status ->
                        when (overlay.key) {
                            StatusOverlayKey.Signer -> StatusOverlay("Signer status", status.signer.text, onIntent)
                            StatusOverlayKey.Sync -> StatusOverlay("Sync status", status.sync.text, onIntent)
                        }
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
        ShellText(overlay.title, Modifier.semantics { heading() }, ShellTextRole.SectionTitle)
        ShellText(overlay.explanation)
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            ShellButton(
                overlay.actionLabel,
                overlay.actionLabel,
                { onIntent(OverlayIntent.Confirm(overlay.action)) },
                Modifier.focusRequester(requester).testTag("overlay-confirm"),
                enabled = !busy,
                kind = ShellButtonKind.Destructive,
            )
            ShellButton(
                "Cancel",
                "Cancel",
                { onIntent(OverlayIntent.DismissConfirmation(overlay.action)) },
                Modifier.testTag("overlay-cancel"),
                !busy,
            )
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
        ShellText(title, Modifier.semantics { heading() }, ShellTextRole.SectionTitle)
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
        ShellText("Open a Nostr reference", Modifier.semantics { heading() }, ShellTextRole.SectionTitle)
        ShellTextField(
            value = overlay.input,
            onValueChange = { onIntent(OverlayIntent.EditReference(it)) },
            label = "Nostr link, event ID, or address",
            placeholder = "nostr:…",
            modifier = Modifier.focusRequester(requester).testTag("nostr-reference-input"),
            enabled = !busy,
        )
        overlay.result?.let { ShellText(it.message, Modifier.testTag("nostr-reference-result")) }
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            ShellButton(
                "Open a Nostr reference",
                "Open a Nostr reference",
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

private fun FoundationOverlay.title(): String =
    when (this) {
        is FoundationOverlay.ConfirmAction -> title
        is FoundationOverlay.Status -> if (key == StatusOverlayKey.Signer) "Signer status" else "Sync status"
        is FoundationOverlay.OpenNostrReference -> "Open a Nostr reference"
    }
