package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HubReferenceRecord
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.application.TraceViewState
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Native interpretation only: no recipient inference, stock math or acknowledgement. */
@Composable
internal fun TraceRecipientCorrections(
    state: TraceViewState,
    presenter: TracePresenter,
) {
    val snapshot = state.snapshot ?: return
    val corrections = snapshot.recipientCorrections
    if (corrections.isEmpty()) return
    HarvestCircleShellText("Linked recipient corrections", role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellText(
        "Original event and as-issued recipients are retained. Each amendment has its own affected scope and outstanding manual follow-up; these records do not prove external delivery or acknowledgement.",
    )
    corrections.drop(state.recipientCorrectionsOffset).take(64).forEach { correction ->
        HarvestCircleShellPanel(Modifier.testTag("trace-recipient-correction-${correction.operationId.value}")) {
            val current = corrections.last { it.originalDepartureOperationId == correction.originalDepartureOperationId }.after
            HarvestCircleShellText("Original recorded recipient: ${traceRecipientMeaning(correction.originalRecipient)}")
            HarvestCircleShellText("Current recipient interpretation for this event: ${traceRecipientMeaning(current)}")
            HarvestCircleShellText("Original event ${correction.originalDepartureOperationId.value}")

            HarvestCircleShellText("Before: ${traceRecipientMeaning(correction.before)}")
            HarvestCircleShellText("After: ${traceRecipientMeaning(correction.after)}")
            HarvestCircleShellText("Basis: ${correction.basis}")
            HarvestCircleShellText("Correction ${correction.operationId.value} · Stock revision ${correction.stockRevision}")
            HarvestCircleShellText("Recorded by ${correction.actor.value} · Recorded time ${correction.recordedAt.value}")
            HarvestCircleShellButton("View affected scope (${correction.impact.size})", {
                presenter.recipientImpact(correction.operationId)
            }, Modifier.testTag("trace-recipient-scope-${correction.operationId.value}"))
        }
    }
    if (corrections.size > 64) {
        HarvestCircleShellText(
            "Showing recipient corrections ${state.recipientCorrectionsOffset + 1}–${minOf(
                state.recipientCorrectionsOffset + 64,
                corrections.size,
            )} of ${corrections.size}",
        )
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellButton(
                "Previous recipient corrections",
                { presenter.recipientCorrectionsPage(-1) },
                Modifier.testTag("trace-recipient-previous"),
                enabled =
                    state.recipientCorrectionsOffset > 0,
            )
            HarvestCircleShellButton(
                "Next recipient corrections",
                { presenter.recipientCorrectionsPage(1) },
                Modifier.testTag("trace-recipient-next"),
                enabled =
                    state.recipientCorrectionsOffset + 64 < corrections.size,
            )
        }
    }
    TraceRecipientImpact(state, presenter)
}

internal fun traceRecipientMeaning(recipient: HubReferenceRecord?): String =
    recipient?.let { "${it.value.label} · ${it.id} · Reference version ${it.revision}" }
        ?: "Unknown recipient; no party is established"

@Composable
private fun TraceRecipientImpact(
    state: TraceViewState,
    presenter: TracePresenter,
) {
    val correction = state.snapshot?.recipientCorrections?.find { it.operationId == state.recipientImpactOperationId } ?: return
    HarvestCircleShellPanel(Modifier.testTag("trace-recipient-affected-scope")) {
        HarvestCircleShellText("Affected scope for ${correction.operationId.value}", role = HarvestCircleShellTextRole.BodyStrong)
        HarvestCircleShellText(
            "Every original and amended recipient, event population and issued version remains manual review scope. Local work is not external acknowledgement.",
        )
        correction.impact.drop(state.recipientImpactOffset).take(64).forEach { impact ->
            HarvestCircleShellText("${traceImpactLabel(impact.kind)} · ${impact.targetId}")
        }
        if (correction.impact.size > 64) {
            HarvestCircleShellText(
                "Showing affected references ${state.recipientImpactOffset + 1}–${minOf(
                    state.recipientImpactOffset + 64,
                    correction.impact.size,
                )} of ${correction.impact.size}",
            )
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                HarvestCircleShellButton(
                    "Previous affected references",
                    { presenter.recipientImpactPage(-1) },
                    Modifier.testTag("trace-recipient-impact-previous"),
                    enabled =
                        state.recipientImpactOffset > 0,
                )
                HarvestCircleShellButton(
                    "Next affected references",
                    { presenter.recipientImpactPage(1) },
                    Modifier.testTag("trace-recipient-impact-next"),
                    enabled =
                        state.recipientImpactOffset + 64 < correction.impact.size,
                )
            }
        }
        HarvestCircleShellButton("Close affected scope", { presenter.recipientImpact(null) })
    }
}
