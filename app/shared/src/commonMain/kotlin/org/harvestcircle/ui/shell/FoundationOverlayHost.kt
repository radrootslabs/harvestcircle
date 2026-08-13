package org.harvestcircle.ui.shell

import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
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
import org.harvestcircle.application.BannerSeverity
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.OverlayState
import org.harvestcircle.application.ShellStatusModel
import org.harvestcircle.application.StatusOverlayKey
import org.harvestcircle.designsystem.component.container.HarvestCircleDialogFrame
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBanner
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBannerTone
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
fun FoundationOverlayHost(
    state: OverlayState,
    status: ShellStatusModel,
    onIntent: (OverlayIntent) -> Unit,
) {
    status.banner?.let { banner ->
        HarvestCircleBanner(
            message = banner.message,
            modifier =
                Modifier
                    .semantics { contentDescription = "Status: ${banner.title}. ${banner.message}" }
                    .testTag("global-status-banner"),
            tone = banner.severity.toBannerTone(),
            title = banner.title,
        )
    }
    state.current?.let { overlay ->
        val overlayBusy = (overlay as? FoundationOverlay.ConfirmAction)?.busy == true
        val rootRequester = remember { FocusRequester() }
        HarvestCircleDialogFrame(
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
private fun ConfirmOverlay(
    overlay: FoundationOverlay.ConfirmAction,
    busy: Boolean,
    rootRequester: FocusRequester,
    onIntent: (OverlayIntent) -> Unit,
) {
    val confirmRequester = remember { FocusRequester() }
    val cancelRequester = remember { FocusRequester() }
    Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap)) {
        ShellText(overlay.title, Modifier.semantics { heading() }, ShellTextRole.SectionTitle)
        ShellText(overlay.explanation)
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            ShellButton(
                overlay.actionLabel,
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
                kind = ShellButtonKind.Destructive,
            )
            ShellButton(
                "Cancel",
                "Cancel",
                { onIntent(OverlayIntent.DismissConfirmation(overlay.action)) },
                Modifier
                    .focusRequester(cancelRequester)
                    .focusProperties {
                        next = confirmRequester
                        previous = confirmRequester
                    }.modalFocusCycle(confirmRequester, confirmRequester)
                    .testTag("overlay-cancel"),
                !busy,
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
    Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap)) {
        ShellText(title, Modifier.semantics { heading() }, ShellTextRole.SectionTitle)
        ShellText(status, Modifier.testTag("overlay-status"))
        ShellButton(
            "Close",
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
    Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap)) {
        ShellText("Open a Nostr reference", Modifier.semantics { heading() }, ShellTextRole.SectionTitle)
        ShellTextField(
            value = overlay.input,
            onValueChange = { onIntent(OverlayIntent.EditReference(it)) },
            label = "Nostr link, note1, nevent1, or address",
            placeholder = "nostr:…",
            modifier =
                Modifier
                    .focusRequester(inputRequester)
                    .focusProperties {
                        next = submitRequester
                        previous = cancelRequester
                    }.modalFocusCycle(submitRequester, cancelRequester)
                    .testTag("nostr-reference-input"),
        )
        overlay.result?.let { ShellText(it.message, Modifier.testTag("nostr-reference-result")) }
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            ShellButton(
                "Open a Nostr reference",
                "Open a Nostr reference",
                { onIntent(OverlayIntent.SubmitReference) },
                Modifier
                    .focusRequester(submitRequester)
                    .focusProperties {
                        next = cancelRequester
                        previous = inputRequester
                    }.modalFocusCycle(cancelRequester, inputRequester)
                    .testTag("nostr-reference-submit"),
                kind = ShellButtonKind.Primary,
            )
            ShellButton(
                "Cancel",
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

private fun BannerSeverity.toBannerTone(): HarvestCircleBannerTone =
    when (this) {
        BannerSeverity.Information -> HarvestCircleBannerTone.Info
        BannerSeverity.Caution -> HarvestCircleBannerTone.Warning
        BannerSeverity.Critical -> HarvestCircleBannerTone.Error
    }
