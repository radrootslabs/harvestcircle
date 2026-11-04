package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HubCorrectionImpactKind
import org.harvestcircle.application.HubSource
import org.harvestcircle.application.HubTraceScope
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.application.TraceViewState
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Native interpretation only: no source inference, stock math or acknowledgement. */
@Composable
internal fun TraceSourceCorrections(
    state: TraceViewState,
    presenter: TracePresenter,
) {
    val snapshot = state.snapshot ?: return
    val corrections = snapshot.sourceCorrections
    if (corrections.isEmpty()) return
    HarvestCircleShellText("Linked source corrections", role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellText(
        "Original recorded source is retained. Each amendment has its own affected scope and outstanding manual follow-up; these records do not prove external delivery or acknowledgement.",
    )
    val current = corrections.last().after
    HarvestCircleShellText("Current source interpretation: ${traceSourceMeaning(current)}", role = HarvestCircleShellTextRole.BodyStrong)
    current.id?.let { source ->
        HarvestCircleShellButton("Find amended source and unresolved candidates", {
            presenter.browse(HubTraceScope.SourceImpact, source)
        }, Modifier.testTag("trace-current-source-impact"), enabled = !state.loading)
    }
    corrections.drop(state.sourceCorrectionsOffset).take(64).forEach { correction ->
        HarvestCircleShellPanel(Modifier.testTag("trace-source-correction-${correction.operationId.value}")) {
            HarvestCircleShellText("Before: ${traceSourceMeaning(correction.before)}")
            HarvestCircleShellText("After: ${traceSourceMeaning(correction.after)}")
            HarvestCircleShellText("Basis: ${correction.basis}")
            HarvestCircleShellText("Correction ${correction.operationId.value} · Stock revision ${correction.stockRevision}")
            HarvestCircleShellText("Recorded by ${correction.actor.value} · Recorded time ${correction.recordedAt.value}")
            HarvestCircleShellButton("View affected scope (${correction.impact.size})", {
                presenter.sourceImpact(correction.operationId)
            }, Modifier.testTag("trace-source-scope-${correction.operationId.value}"))
        }
    }
    if (corrections.size > 64) {
        HarvestCircleShellText(
            "Showing source corrections ${state.sourceCorrectionsOffset + 1}–${minOf(
                state.sourceCorrectionsOffset + 64,
                corrections.size,
            )} of ${corrections.size}",
        )
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellButton(
                "Previous source corrections",
                { presenter.sourceCorrectionsPage(-1) },
                Modifier.testTag("trace-source-previous"),
                enabled =
                    state.sourceCorrectionsOffset > 0,
            )
            HarvestCircleShellButton(
                "Next source corrections",
                { presenter.sourceCorrectionsPage(1) },
                Modifier.testTag("trace-source-next"),
                enabled =
                    state.sourceCorrectionsOffset + 64 < corrections.size,
            )
        }
    }
    TraceSourceImpact(state, presenter)
}

internal fun traceSourceMeaning(source: HubSource): String =
    if (source.id == null) {
        "Unknown source; no relationship is established"
    } else {
        "${source.code} · ${source.id} · Issuer ${source.issuer?.value?.label ?: "Unknown"} · ${source.certainty}"
    }

@Composable
private fun TraceSourceImpact(
    state: TraceViewState,
    presenter: TracePresenter,
) {
    val correction = state.snapshot?.sourceCorrections?.find { it.operationId == state.sourceImpactOperationId } ?: return
    HarvestCircleShellPanel(Modifier.testTag("trace-source-affected-scope")) {
        HarvestCircleShellText("Affected scope for ${correction.operationId.value}", role = HarvestCircleShellTextRole.BodyStrong)
        HarvestCircleShellText(
            "Original and amended populations, sources, departures, issued versions and recipients remain review scope. Local work is not external acknowledgement.",
        )
        correction.impact.drop(state.sourceImpactOffset).take(64).forEach { impact ->
            HarvestCircleShellText("${traceImpactLabel(impact.kind)} · ${impact.targetId}")
        }
        if (correction.impact.size > 64) {
            HarvestCircleShellText(
                "Showing affected references ${state.sourceImpactOffset + 1}–${minOf(
                    state.sourceImpactOffset + 64,
                    correction.impact.size,
                )} of ${correction.impact.size}",
            )
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                HarvestCircleShellButton(
                    "Previous affected references",
                    { presenter.sourceImpactPage(-1) },
                    Modifier.testTag("trace-source-impact-previous"),
                    enabled =
                        state.sourceImpactOffset > 0,
                )
                HarvestCircleShellButton(
                    "Next affected references",
                    { presenter.sourceImpactPage(1) },
                    Modifier.testTag("trace-source-impact-next"),
                    enabled =
                        state.sourceImpactOffset + 64 < correction.impact.size,
                )
            }
        }
        HarvestCircleShellButton("Close affected scope", { presenter.sourceImpact(null) })
    }
}

internal fun traceImpactLabel(kind: HubCorrectionImpactKind): String =
    when (kind) {
        HubCorrectionImpactKind.Population -> "Population"
        HubCorrectionImpactKind.Source -> "Source"
        HubCorrectionImpactKind.Departure -> "Departure"
        HubCorrectionImpactKind.IssuedVersion -> "Issued version"
        HubCorrectionImpactKind.Recipient -> "Recipient"
        HubCorrectionImpactKind.UnresolvedSource -> "Unresolved source for population"
        HubCorrectionImpactKind.UnresolvedRecipient -> "Unresolved recipient for departure"
    }
