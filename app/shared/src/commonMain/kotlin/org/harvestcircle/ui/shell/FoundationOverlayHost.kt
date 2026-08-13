package org.harvestcircle.ui.shell

import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.BannerSeverity
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.OverlayState
import org.harvestcircle.application.ShellStatusModel
import org.harvestcircle.application.StatusOverlayKey
import org.harvestcircle.designsystem.shell.HarvestCircleShellBanner
import org.harvestcircle.designsystem.shell.HarvestCircleShellBannerTone
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellDialogFrame
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole

@Composable
fun FoundationOverlayHost(
    state: OverlayState,
    status: ShellStatusModel,
    showBanner: Boolean = true,
    onIntent: (OverlayIntent) -> Unit,
) {
    if (showBanner) {
        FoundationStatusBanner(status)
    }
    state.current?.let { overlay ->
        val overlayBusy = (overlay as? FoundationOverlay.ConfirmAction)?.busy == true
        val rootRequester = remember { FocusRequester() }
        HarvestCircleShellDialogFrame(
            onDismissRequest = {
                if (!overlayBusy) {
                    if (overlay is FoundationOverlay.ConfirmAction) {
                        onIntent(OverlayIntent.DismissConfirmation(overlay.action))
                    } else {
                        onIntent(OverlayIntent.Close)
                    }
                }
            },
            title = overlay.title(),
            modifier =
                Modifier
                    .focusGroup()
                    .focusRequester(rootRequester)
                    .focusable(overlayBusy)
                    .semantics { contentDescription = "Dialog: ${overlay.title()}" }
                    .testTag("foundation-overlay"),
        ) {
            when (overlay) {
                is FoundationOverlay.ConfirmAction -> ConfirmOverlay(overlay, overlayBusy, rootRequester, onIntent)
                is FoundationOverlay.Status ->
                    when (overlay.key) {
                        StatusOverlayKey.Signer -> StatusOverlay("Signer status", status.signer.text, onIntent)
                        StatusOverlayKey.Sync -> StatusOverlay("Sync status", status.sync.text, onIntent)
                    }
                is FoundationOverlay.OpenNostrReference -> ReferenceOverlay(overlay, onIntent)
            }
        }
    }
}

@Composable
fun FoundationStatusBanner(
    status: ShellStatusModel,
    modifier: Modifier = Modifier,
) {
    status.banner?.let { banner ->
        HarvestCircleShellBanner(
            message = banner.message,
            modifier =
                modifier
                    .padding(16.dp)
                    .semantics { contentDescription = "Status: ${banner.title}. ${banner.message}" }
                    .testTag("global-status-banner"),
            tone = banner.severity.toShellBannerTone(),
            title = banner.title,
        )
    }
}

@Composable
private fun ConfirmOverlay(
    overlay: FoundationOverlay.ConfirmAction,
    busy: Boolean,
    rootRequester: FocusRequester,
    onIntent: (OverlayIntent) -> Unit,
) {
    val confirmRequester = remember { FocusRequester() }
    val cancelRequester = remember { FocusRequester() }
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        HarvestCircleShellText(overlay.title, Modifier.semantics { heading() }, HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText(overlay.explanation)
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            HarvestCircleShellButton(
                overlay.actionLabel,
                { onIntent(OverlayIntent.Confirm(overlay.action)) },
                Modifier
                    .focusRequester(confirmRequester)
                    .focusProperties {
                        next = cancelRequester
                        previous = cancelRequester
                    }.modalFocusCycle(cancelRequester, cancelRequester)
                    .testTag("overlay-confirm"),
                enabled = !busy,
                destructive = true,
            )
            HarvestCircleShellButton(
                "Cancel",
                { onIntent(OverlayIntent.DismissConfirmation(overlay.action)) },
                Modifier
                    .focusRequester(cancelRequester)
                    .focusProperties {
                        next = confirmRequester
                        previous = confirmRequester
                    }.modalFocusCycle(confirmRequester, confirmRequester)
                    .testTag("overlay-cancel"),
                enabled = !busy,
            )
        }
    }
    LaunchedEffect(busy) {
        if (busy) rootRequester.requestFocus() else confirmRequester.requestFocus()
    }
}

@Composable
private fun StatusOverlay(
    title: String,
    status: String,
    onIntent: (OverlayIntent) -> Unit,
) {
    val requester = remember { FocusRequester() }
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        HarvestCircleShellText(title, Modifier.semantics { heading() }, HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText(status, Modifier.testTag("overlay-status"))
        HarvestCircleShellButton(
            "Close",
            { onIntent(OverlayIntent.Close) },
            Modifier
                .focusRequester(requester)
                .focusProperties {
                    next = requester
                    previous = requester
                }.modalFocusCycle(requester, requester)
                .testTag("overlay-close"),
        )
    }
    LaunchedEffect(Unit) { requester.requestFocus() }
}

@Composable
private fun ReferenceOverlay(
    overlay: FoundationOverlay.OpenNostrReference,
    onIntent: (OverlayIntent) -> Unit,
) {
    val inputRequester = remember { FocusRequester() }
    val submitRequester = remember { FocusRequester() }
    val cancelRequester = remember { FocusRequester() }
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        HarvestCircleShellText(
            "Open a Nostr reference",
            Modifier.semantics { heading() },
            HarvestCircleShellTextRole.SectionTitle,
        )
        HarvestCircleShellTextField(
            value = overlay.input,
            onValueChange = { onIntent(OverlayIntent.EditReference(it)) },
            label = "Nostr link, note1, nevent1, or address",
            placeholder = "nostr:…",
            inputModifier =
                Modifier
                    .focusRequester(inputRequester)
                    .focusProperties {
                        next = submitRequester
                        previous = cancelRequester
                    }.modalFocusCycle(submitRequester, cancelRequester)
                    .testTag("nostr-reference-input"),
        )
        overlay.result?.let { HarvestCircleShellText(it.message, Modifier.testTag("nostr-reference-result")) }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            HarvestCircleShellButton(
                "Open a Nostr reference",
                { onIntent(OverlayIntent.SubmitReference) },
                Modifier
                    .focusRequester(submitRequester)
                    .focusProperties {
                        next = cancelRequester
                        previous = inputRequester
                    }.modalFocusCycle(cancelRequester, inputRequester)
                    .testTag("nostr-reference-submit"),
                primary = true,
            )
            HarvestCircleShellButton(
                "Cancel",
                { onIntent(OverlayIntent.Close) },
                Modifier
                    .focusRequester(cancelRequester)
                    .focusProperties {
                        next = inputRequester
                        previous = submitRequester
                    }.modalFocusCycle(inputRequester, submitRequester)
                    .testTag("overlay-cancel"),
            )
        }
    }
    LaunchedEffect(Unit) { inputRequester.requestFocus() }
}

private fun Modifier.modalFocusCycle(
    next: FocusRequester,
    previous: FocusRequester,
): Modifier =
    onPreviewKeyEvent { event ->
        if (event.type == KeyEventType.KeyDown && event.key == Key.Tab) {
            (if (event.isShiftPressed) previous else next).requestFocus()
            true
        } else {
            false
        }
    }

private fun FoundationOverlay.title(): String =
    when (this) {
        is FoundationOverlay.ConfirmAction -> title
        is FoundationOverlay.Status -> if (key == StatusOverlayKey.Signer) "Signer status" else "Sync status"
        is FoundationOverlay.OpenNostrReference -> "Open a Nostr reference"
    }

private fun BannerSeverity.toShellBannerTone(): HarvestCircleShellBannerTone =
    when (this) {
        BannerSeverity.Information -> HarvestCircleShellBannerTone.Information
        BannerSeverity.Caution -> HarvestCircleShellBannerTone.Caution
        BannerSeverity.Critical -> HarvestCircleShellBannerTone.Critical
    }
