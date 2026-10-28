package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HandlingDraft
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HubAcceptanceStatus
import org.harvestcircle.application.TreatmentEditorAction
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellText

internal fun treatmentTitle(action: TreatmentEditorAction) =
    when (action) {
        TreatmentEditorAction.Acceptance -> "Record sample acceptance"
        TreatmentEditorAction.PlaceHold -> "Place independent hold"
        TreatmentEditorAction.ReleaseHold -> "Release one complete hold"
    }

@Composable
internal fun TreatmentDraftScope(draft: HandlingDraft.Treatment) {
    HarvestCircleShellText("Receipt: ${draft.selected.trace.id}")
    HarvestCircleShellText("Captured stock revision ${draft.versions.stock} · Catalog ${draft.versions.catalog}")
    HarvestCircleShellText(
        "Sample basis: ${draft.selected.basis.id} v${draft.selected.basis.version} · ${draft.selected.basis.siteTimezone}",
    )
    if (draft.action == TreatmentEditorAction.ReleaseHold) {
        val row = draft.selected.holds.find { it.hold.stamp.id == draft.fields.holdId }
        HarvestCircleShellText(
            row?.let { "Selected hold: ${it.hold.stamp.id} · Current scope ${it.currentWholePacks} original packs" }
                ?: "Select the complete hold to release.",
        )
        row?.let { TreatmentStamp(it.hold.stamp) }
    } else {
        HarvestCircleShellText("Selected whole groups: ${draft.fields.treatmentGroups.size}")
        draft.fields.treatmentGroups.forEach { id ->
            val group = draft.selected.groups.find { it.groupId == id }
            HarvestCircleShellText(treatmentGroupLabel(draft, id))
            HarvestCircleShellText(group?.let { "${it.wholePacks} original packs · $id" } ?: "No longer on hand in the reviewed scope: $id")
        }
    }
}

@Composable
internal fun TreatmentFields(
    draft: HandlingDraft.Treatment,
    presenter: HandlingPresenter,
) {
    val fields = draft.fields
    if (draft.action == TreatmentEditorAction.ReleaseHold) {
        HarvestCircleShellText("Release selects one entire hold and does not grant acceptance or clear any other hold.")
        draft.selected.holds.forEach { row ->
            HarvestCircleShellText(row.hold.stamp.observation)
            HarvestCircleShellButton(
                "${if (row.hold.release == null) "Select hold" else "Released hold"} ${row.hold.stamp.id} · ${row.currentWholePacks} packs",
                { presenter.edit(fields.copy(holdId = row.hold.stamp.id)) },
                Modifier.testTag("treatment-hold-${row.hold.stamp.id}"),
                primary = fields.holdId == row.hold.stamp.id,
                enabled =
                    row.hold.release == null,
            )
        }
    } else {
        HarvestCircleShellText(
            "Select whole identified groups from this receipt, up to 64. To treat part of a group, first record its observed physical separation.",
        )
        draft.selected.groups.forEach { group ->
            val selected = group.groupId in fields.treatmentGroups
            HarvestCircleShellText(treatmentGroupLabel(draft, group.groupId))
            HarvestCircleShellButton(
                "${if (selected) "Selected" else "Select"} ${group.wholePacks} original packs · ${group.groupId}",
                {
                    presenter.edit(
                        fields.copy(
                            treatmentGroups =
                                if (selected) {
                                    fields.treatmentGroups - group.groupId
                                } else {
                                    fields.treatmentGroups +
                                        group.groupId
                                },
                        ),
                    )
                },
                Modifier.testTag("treatment-group-${group.groupId}"),
                primary = selected,
                enabled =
                    selected || fields.treatmentGroups.size < 64,
            )
        }
        if (draft.action == TreatmentEditorAction.Acceptance) {
            HarvestCircleShellText("Choose the explicit sample acceptance decision. This does not release holds.")
            HubAcceptanceStatus.entries.forEach { status ->
                HarvestCircleShellButton(
                    acceptanceLabel(status),
                    { presenter.edit(fields.copy(acceptanceStatus = status)) },
                    Modifier.testTag("treatment-status-$status"),
                    primary =
                        fields.acceptanceStatus == status,
                )
            }
        } else {
            HarvestCircleShellText(
                "This is an independent precautionary hold. It blocks normal recipient departure under the sample basis; internal relocation may remain allowed.",
            )
        }
    }
}

internal fun acceptanceLabel(status: HubAcceptanceStatus) =
    when (status) {
        HubAcceptanceStatus.ReviewRequired -> "Review required"
        HubAcceptanceStatus.Accepted -> "Accepted"
        HubAcceptanceStatus.Rejected -> "Rejected"
    }

private fun treatmentGroupLabel(
    draft: HandlingDraft.Treatment,
    id: String,
): String {
    val row = draft.choices.find { it.id == id } ?: return "Physical identification unavailable"
    return "${row.identification?.label ?: "Original received group"} · ${row.location?.value?.label ?: "No current location"}"
}
