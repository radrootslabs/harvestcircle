package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HandlingAction
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HandlingViewState
import org.harvestcircle.application.HubHandlingResult
import org.harvestcircle.application.HubTreatmentChange
import org.harvestcircle.application.HubTreatmentSnapshot
import org.harvestcircle.application.HubTreatmentStamp
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole

@Composable
internal fun TreatmentRecord(
    snapshot: HubTreatmentSnapshot,
    state: HandlingViewState,
    presenter: HandlingPresenter,
) {
    HarvestCircleShellPanel(Modifier.testTag("treatment-record")) {
        HarvestCircleShellText("Sample acceptance and holds", role = HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText("Receipt population · Stock ${snapshot.stockRevision} · Catalog ${snapshot.catalogRevision}")
        val counts = snapshot.summary
        HarvestCircleShellText(
            "On hand ${counts.onHand} · Accepted ${counts.accepted} · Review required ${counts.reviewRequired} · Rejected ${counts.rejected}",
        )
        HarvestCircleShellText(
            "Restricted ${counts.restricted} · Eligible physical on hand ${counts.departureEligible}",
            Modifier.testTag("treatment-summary"),
        )
        HarvestCircleShellText(
            "Sample basis: ${snapshot.basis.label} · ${snapshot.basis.id} v${snapshot.basis.version} · ${snapshot.basis.siteTimezone}",
        )
        HarvestCircleShellText(
            "These native counts describe current physical treatment. A future departure still requires fresh native checks.",
        )
        if (snapshot.historyCapacityReached) {
            HarvestCircleShellText(
                "Treatment history capacity reached. Normal departure is blocked; existing hold releases remain available.",
            )
        }
        if (!snapshot.normalOperationsAvailable) {
            HarvestCircleShellText(
                "Normal operation capacity is unavailable. Physical counts above are unchanged; a selected release is checked separately.",
            )
        }
        snapshot.groups.find { it.groupId == state.group?.group?.id }?.let { group ->
            HarvestCircleShellText("Selected group: ${acceptanceLabel(group.acceptance)} · Active holds ${group.activeHoldIds.size}")
            HarvestCircleShellText("Current sample departure check: ${if (group.actions.normalDeparture) "Ready" else "Blocked"}")
            group.activeHoldIds.forEach { HarvestCircleShellText("Active hold: $it") }
        }
        HarvestCircleShellButton("Record sample acceptance", {
            presenter.begin(HandlingAction.SetAcceptance)
        }, Modifier.testTag("treatment-begin-acceptance"), enabled = !state.loading)
        HarvestCircleShellButton("Place independent hold", {
            presenter.begin(HandlingAction.PlaceHold)
        }, Modifier.testTag("treatment-begin-hold"), enabled = !state.loading)
        HarvestCircleShellButton(
            "Release one complete hold",
            { presenter.begin(HandlingAction.ReleaseHold) },
            Modifier.testTag("treatment-begin-release"),
            enabled =
                !state.loading && snapshot.holds.any { it.hold.release == null },
        )
    }
    snapshot.holds.forEach { row ->
        HarvestCircleShellPanel(Modifier.testTag("treatment-hold-record-${row.hold.stamp.id}")) {
            HarvestCircleShellText(
                if (row.hold.release ==
                    null
                ) {
                    "Active independent hold"
                } else {
                    "Released independent hold"
                },
                role = HarvestCircleShellTextRole.BodyStrong,
            )
            HarvestCircleShellText("Hold: ${row.hold.stamp.id} · Current population ${row.currentWholePacks} original packs")
            TreatmentStamp(row.hold.stamp)
            TreatmentScope(row.hold.scope)
            row.hold.release?.let { release ->
                HarvestCircleShellText("Selected release retained")
                TreatmentStamp(release)
            }
            HarvestCircleShellText("Original placement operation: ${row.createdOperationId.value}")
            row.releasedOperationId?.let { HarvestCircleShellText("Original release operation: ${it.value}") }
        }
    }
    TreatmentHistory(state, presenter)
}

@Composable
private fun TreatmentHistory(
    state: HandlingViewState,
    presenter: HandlingPresenter,
) {
    state.treatmentHistory?.let { page ->
        HarvestCircleShellText(
            "${page.total} original treatment operations · Page offset ${page.offset}",
            role = HarvestCircleShellTextRole.BodyStrong,
        )
        page.rows.forEach { receipt ->
            val result = receipt.result as? HubHandlingResult.Treatment
            if (result != null) {
                HarvestCircleShellPanel(Modifier.testTag("treatment-history-${receipt.operationId.value}")) {
                    HarvestCircleShellText("Original operation: ${receipt.operationId.value}")
                    TreatmentChangeSummary(result.value.change)
                    HarvestCircleShellText("Affected when recorded: ${result.value.affectedWholePacks} original packs")
                    HarvestCircleShellText("This original is retained history. Current treatment is shown above.")
                }
            }
        }
        if (page.offset >
            0U
        ) {
            HarvestCircleShellButton("First treatment records", {
                state.group?.let { presenter.inspectGroup(it.group.id) }
            }, Modifier.testTag("treatment-first"), enabled = !state.loading)
        }
        if (page.more) {
            HarvestCircleShellButton(
                "Next treatment records",
                presenter::nextTreatmentHistoryPage,
                Modifier.testTag("treatment-next"),
                enabled = !state.loading,
            )
        }
    }
}

@Composable
internal fun TreatmentStamp(stamp: HubTreatmentStamp) {
    HarvestCircleShellText("Observation and basis: ${stamp.observation}")
    HarvestCircleShellText("Recorded: ${stockUtc(stamp.recordedAt.value)} · ${stamp.actor.value} · Stock ${stamp.stockRevision}")
    HarvestCircleShellText("Sample basis: ${stamp.basis.id} v${stamp.basis.version} · ${stamp.basis.siteTimezone}")
    HarvestCircleShellText("Actual occurrence: ${stamp.occurrence.kind}")
    stamp.occurrence.startUnixSeconds?.let { HarvestCircleShellText("Occurrence start: ${stockUtc(it)}") }
    stamp.occurrence.endUnixSeconds?.let { HarvestCircleShellText("Occurrence end: ${stockUtc(it)}") }
}

@Composable
private fun TreatmentScope(scope: org.harvestcircle.application.HubCapturedPhysicalScope) {
    HarvestCircleShellText("Original scope captured at stock ${scope.capturedStockRevision}")
    scope.anchors.forEach {
        HarvestCircleShellText(
            "${it.capturedWholePacks} original packs · Group ${it.groupId} · Created at stock ${it.createdStockRevision}",
        )
    }
}

@Composable
internal fun TreatmentChangeSummary(change: HubTreatmentChange) {
    when (change) {
        is HubTreatmentChange.Acceptance -> {
            HarvestCircleShellText("Sample decision: ${acceptanceLabel(change.value.status)}")
            TreatmentStamp(change.value.stamp)
            TreatmentScope(change.value.scope)
        }
        is HubTreatmentChange.HoldPlaced -> {
            HarvestCircleShellText("Independent hold: ${change.value.stamp.id}")
            TreatmentStamp(change.value.stamp)
            TreatmentScope(change.value.scope)
        }
        is HubTreatmentChange.HoldReleased -> {
            HarvestCircleShellText("Released only hold: ${change.holdId}")
            TreatmentStamp(change.release)
        }
    }
}
