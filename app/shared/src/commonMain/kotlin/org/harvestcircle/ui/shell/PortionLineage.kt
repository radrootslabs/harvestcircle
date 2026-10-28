package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HubGroupCreationKind
import org.harvestcircle.application.HubLineagePage
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole

@Composable
internal fun PortionLineage(
    page: HubLineagePage,
    loading: Boolean,
    presenter: HandlingPresenter,
    selectedGroupId: String,
) {
    HarvestCircleShellText(
        "Physical portions · ${page.total} groups",
        Modifier.testTag("portion-lineage"),
        HarvestCircleShellTextRole.SectionTitle,
    )
    HarvestCircleShellText(
        "These records retain the original population and each observed separation. Moving a whole group keeps its identity.",
    )
    page.rows.forEach { row ->
        HarvestCircleShellPanel(Modifier.testTag("portion-row-${row.id}")) {
            HarvestCircleShellText(row.identification?.label ?: "Original received group", role = HarvestCircleShellTextRole.BodyStrong)
            HarvestCircleShellText("Group: ${row.id}")
            row.parentId?.let { HarvestCircleShellText("Separated from: $it") }
            HarvestCircleShellText("On hand: ${row.wholePacks} original packs")
            HarvestCircleShellText(
                row.location?.let { "At: ${it.value.label}" } ?: "No stock remains in this group; its history is retained.",
            )
            val creation =
                when (row.createdKind) {
                    HubGroupCreationKind.Receipt -> "Received"
                    HubGroupCreationKind.Movement -> "Separated during movement"
                    HubGroupCreationKind.Separation -> "Separated at the same location"
                }
            HarvestCircleShellText(creation)
            HarvestCircleShellText("Recorded: ${stockUtc(row.createdAt.value)} · ${row.createdBy.value}")
            HarvestCircleShellText("Actual occurrence: ${row.occurrence.kind}")
            row.occurrence.startUnixSeconds?.let { HarvestCircleShellText("Occurrence start: ${stockUtc(it)}") }
            row.occurrence.endUnixSeconds?.let { HarvestCircleShellText("Occurrence end: ${stockUtc(it)}") }
            HarvestCircleShellText("Original operation: ${row.createdOperationId.value}")
            row.identification?.let { HarvestCircleShellText("Identification observation: ${it.observation}") }
            HarvestCircleShellText("Observation: ${row.observation}")
            if (row.wholePacks > 0U) {
                HarvestCircleShellButton("View current group", {
                    presenter.inspectGroup(row.id)
                }, Modifier.testTag("portion-open-${row.id}"), enabled = !loading)
            }
        }
    }
    if (page.offset >
        0U
    ) {
        HarvestCircleShellButton("First portion records", {
            presenter.inspectGroup(selectedGroupId)
        }, Modifier.testTag("portion-first"), enabled = !loading)
    }
    if (page.more) {
        HarvestCircleShellButton(
            "Next portion records",
            presenter::nextLineagePage,
            Modifier.testTag("portion-next"),
            enabled = !loading,
        )
    }
}
