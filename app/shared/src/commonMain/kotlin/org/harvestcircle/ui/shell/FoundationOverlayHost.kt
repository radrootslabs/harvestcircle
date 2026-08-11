package org.harvestcircle.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.OverlayState

@Composable
fun FoundationOverlayHost(
    state: OverlayState,
    onIntent: (OverlayIntent) -> Unit,
) {
    state.banner?.let { banner ->
        BasicText(
            banner.message,
            Modifier.semantics { contentDescription = "Status: ${banner.message}" }.testTag("global-status-banner"),
        )
    }
    state.current?.let { overlay ->
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Column(
                Modifier
                    .background(LocalHarvestCirclePalette.current.surface.toComposeColor())
                    .focusGroup()
                    .semantics { contentDescription = "Dialog" }
                    .testTag("foundation-overlay"),
            ) {
                when (overlay) {
                    is FoundationOverlay.ConfirmAction -> ConfirmOverlay(overlay, onIntent)
                    is FoundationOverlay.SignerStatus -> StatusOverlay("Signer status", overlay.status.text, onIntent)
                    is FoundationOverlay.SyncStatus -> StatusOverlay("Sync status", overlay.status.text, onIntent)
                    is FoundationOverlay.OpenNostrReference -> ReferenceOverlay(overlay, onIntent)
                }
            }
        }
    }
}

@Composable
private fun ConfirmOverlay(
    overlay: FoundationOverlay.ConfirmAction,
    onIntent: (OverlayIntent) -> Unit,
) {
    BasicText(overlay.title)
    BasicText(overlay.explanation)
    ShellAction(overlay.actionLabel, overlay.actionLabel, "overlay-confirm") { onIntent(OverlayIntent.Confirm) }
    ShellAction("Cancel", "Cancel", "overlay-cancel") { onIntent(OverlayIntent.Close) }
}

@Composable
private fun StatusOverlay(
    title: String,
    status: String,
    onIntent: (OverlayIntent) -> Unit,
) {
    BasicText(title)
    BasicText(status, Modifier.testTag("overlay-status"))
    ShellAction("Close", "Close", "overlay-close") { onIntent(OverlayIntent.Close) }
}

@Composable
private fun ReferenceOverlay(
    overlay: FoundationOverlay.OpenNostrReference,
    onIntent: (OverlayIntent) -> Unit,
) {
    val requester = androidx.compose.runtime.remember { FocusRequester() }
    BasicText("Open a Nostr reference")
    BasicTextField(
        value = overlay.input,
        onValueChange = { onIntent(OverlayIntent.EditReference(it)) },
        modifier = Modifier.focusRequester(requester).testTag("nostr-reference-input"),
    )
    androidx.compose.runtime.LaunchedEffect(Unit) { requester.requestFocus() }
    overlay.result?.let { BasicText(it.message, Modifier.testTag("nostr-reference-result")) }
    ShellAction("Open reference", "Open Nostr reference", "nostr-reference-submit") {
        onIntent(OverlayIntent.SubmitReference)
    }
    ShellAction("Cancel", "Cancel", "overlay-cancel") { onIntent(OverlayIntent.Close) }
}
