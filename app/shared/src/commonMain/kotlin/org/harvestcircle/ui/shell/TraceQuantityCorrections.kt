package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HubTraceQuantityAmendmentKind
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.application.TraceViewState
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Displays native original/current facts; no stock arithmetic or posting action. */
@Composable
internal fun TraceQuantityCorrections(
    state: TraceViewState,
    presenter: TracePresenter,
) {
    val snapshot = state.snapshot ?: return
    if (snapshot.quantityCorrections.isEmpty()) return
    val rows =
        remember(snapshot.quantityCorrections) {
            snapshot.quantityCorrections
                .flatMap { correction ->
                    correction.amendments.map { correction to it }
                }.sortedBy { it.second.stockRevision }
        }
    HarvestCircleShellText("Linked quantity corrections", role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellText(
        "Original departure and issued records are retained. Recipient and document follow-up needs an explicit record of manual work.",
    )
    rows.drop(state.quantityCorrectionsOffset).take(64).forEach { (correction, amendment) ->
        HarvestCircleShellPanel(Modifier.testTag("trace-correction-${amendment.operationId.value}")) {
            HarvestCircleShellText(
                "Original ${correction.originalWholePacks} · Current ${correction.effectiveWholePacks} · Established returns ${correction.establishedReturnWholePacks}",
                role = HarvestCircleShellTextRole.BodyStrong,
            )
            val kind =
                when (amendment.kind) {
                    HubTraceQuantityAmendmentKind.AdditionalDeparture -> "Additional departure"
                    HubTraceQuantityAmendmentKind.IdentifiedFoundRemainder -> "Independently identified found remainder"
                }
            HarvestCircleShellText(
                "$kind ${amendment.deltaWholePacks} · Before ${amendment.beforeWholePacks} · After ${amendment.afterWholePacks}",
            )
            HarvestCircleShellText("Original operation ${correction.originalOperationId.value} · Group ${correction.originalGroupId}")
            HarvestCircleShellText("Correction ${amendment.operationId.value} · Recorded stock revision ${amendment.stockRevision}")
        }
    }
    if (rows.size > 64) {
        HarvestCircleShellText(
            "Showing ${state.quantityCorrectionsOffset + 1}–${minOf(
                state.quantityCorrectionsOffset + 64,
                rows.size,
            )} of ${rows.size} correction records",
        )
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellButton(
                "Previous corrections",
                { presenter.quantityCorrectionsPage(-1) },
                Modifier.testTag("trace-corrections-previous"),
                enabled =
                    state.quantityCorrectionsOffset > 0,
            )
            HarvestCircleShellButton(
                "Next corrections",
                { presenter.quantityCorrectionsPage(1) },
                Modifier.testTag("trace-corrections-next"),
                enabled =
                    state.quantityCorrectionsOffset + 64 < rows.size,
            )
        }
    }
}
