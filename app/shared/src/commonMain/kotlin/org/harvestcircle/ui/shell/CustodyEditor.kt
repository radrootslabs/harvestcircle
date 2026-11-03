package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.CustodyDraft
import org.harvestcircle.application.CustodyPresenter
import org.harvestcircle.application.CustodyTarget
import org.harvestcircle.application.HubDispositionKind
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubSourceCertainty
import org.harvestcircle.application.HubSourceKind
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMenuButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText

@Composable
internal fun CustodyEditor(
    target: CustodyTarget,
    draft: CustodyDraft,
    presenter: CustodyPresenter,
) {
    val fields = draft.fields
    val common = fields.common
    if (target == CustodyTarget.Return) {
        ReferenceChoice(
            "Returned pack",
            "custody-variant",
            draft.context.references,
            HubReferenceKind.Pack,
            common.variantId,
            { id -> presenter.edit { it.copy(common = it.common.copy(variantId = id)) } },
            optional = false,
        )
        ReferenceChoice(
            "Returned at",
            "custody-location",
            draft.context.references,
            HubReferenceKind.Location,
            common.locationId,
            { id -> presenter.edit { it.copy(common = it.common.copy(locationId = id)) } },
            optional = false,
        )
        ReferenceChoice(
            "Returning party (independent of original recipient)",
            "custody-returned-by",
            draft.context.references,
            HubReferenceKind.Party,
            fields.returnedById,
            { id -> presenter.edit { it.copy(returnedById = id) } },
        )
        ReferenceChoice(
            "New handling target (optional fresh pending work)",
            "custody-planned-location",
            draft.context.references,
            HubReferenceKind.Location,
            fields.plannedLocationId,
            { id -> presenter.edit { it.copy(plannedLocationId = id) } },
        )
        CustodyField("Actually returned whole packs", "custody-quantity", common.quantity) { value ->
            presenter.edit {
                it.copy(common = it.common.copy(quantity = value))
            }
        }
        CustodyField("Physical identification", "custody-label", common.physicalLabel) { value ->
            presenter.edit {
                it.copy(common = it.common.copy(physicalLabel = value))
            }
        }
        CustodyField(
            "How these returned packs are distinguished",
            "custody-identification",
            common.physicalObservation,
        ) { value ->
            presenter.edit {
                it.copy(common = it.common.copy(physicalObservation = value))
            }
        }
        CustodySource(draft, presenter)
        CustodyAssociation(draft, presenter)
        HarvestCircleShellText(
            "This records newly observed custody. It does not restore old work, acceptance, holds, delivery or source knowledge.",
        )
    } else {
        val group = requireNotNull(draft.context.group)
        HarvestCircleShellPanel {
            HarvestCircleShellText("Complete selected physical group: ${group.group.wholePacks} whole original packs")
            HarvestCircleShellText("${group.product.value.label} · ${group.variant.value.label} · ${group.location.value.label}")
            HarvestCircleShellText(group.group.identification?.label ?: "Original group without separate identification")
            HarvestCircleShellText("Group ${group.group.id} · Version ${group.group.revision}")
            group.group.work.forEach {
                HarvestCircleShellText(
                    "${it.progress} work ${it.requirement.id} · ${it.requirement.quantity.required} required · ${it.requirement.quantity.completed} completed · ${it.requirement.quantity.remaining} pending",
                )
            }
        }
        HarvestCircleShellText(
            "Only this complete observed group is removed. To remove a smaller quantity, return to Handling and physically separate the observed portion first. No drafting or paper action removes stock.",
        )
        HarvestCircleShellMenuButton(
            fields.dispositionKind,
            HubDispositionKind.entries.map {
                HarvestCircleMenuOption(
                    it,
                    if (it == HubDispositionKind.Destroyed) "Destroyed" else "Other observed physical removal",
                )
            },
            { kind -> presenter.edit { it.copy(dispositionKind = kind) } },
            if (fields.dispositionKind ==
                HubDispositionKind.Destroyed
            ) {
                "Destroyed"
            } else {
                "Other observed physical removal"
            },
            Modifier.testTag("custody-disposition-kind"),
        )
        HarvestCircleShellText(
            "Pending work for removed scope retires. Completed/retired work and holds remain historical facts. This is not buyer departure, unexplained count loss or a refund.",
        )
    }
    HarvestCircleShellMenuButton(
        common.cutoffKind,
        HubOccurrenceKind.entries.map { HarvestCircleMenuOption(it, it.name) },
        { kind ->
            presenter.edit { it.copy(common = it.common.copy(cutoffKind = kind)) }
        },
        common.cutoffKind.name,
        Modifier.testTag("custody-occurrence-kind"),
    )
    if (common.cutoffKind != HubOccurrenceKind.Unknown) {
        CustodyField("Actual occurrence (UTC)", "custody-occurrence-start", common.cutoffStart) { value ->
            presenter.edit {
                it.copy(common = it.common.copy(cutoffStart = value))
            }
        }
    }
    if (common.cutoffKind == HubOccurrenceKind.Interval) {
        CustodyField("Latest actual occurrence (UTC)", "custody-occurrence-end", common.cutoffEnd) { value ->
            presenter.edit {
                it.copy(common = it.common.copy(cutoffEnd = value))
            }
        }
    }
    HarvestCircleShellText("Unknown actual time remains unknown. Recording time is separate.")
    CustodyField("Observed physical facts and provenance", "custody-observation", common.observation) { value ->
        presenter.edit {
            it.copy(common = it.common.copy(observation = value))
        }
    }
}

@Composable
private fun CustodySource(
    draft: CustodyDraft,
    presenter: CustodyPresenter,
) {
    val common = draft.fields.common
    HarvestCircleShellText("Independent returned-stock source knowledge")
    HarvestCircleShellMenuButton(
        common.sourceCertainty,
        HubSourceCertainty.entries.map { HarvestCircleMenuOption(it, it.name) },
        { certainty ->
            presenter.edit { it.copy(common = it.common.copy(sourceCertainty = certainty)) }
        },
        common.sourceCertainty.name,
        Modifier.testTag("custody-source-certainty"),
    )
    if (common.sourceCertainty != HubSourceCertainty.Unknown) {
        HarvestCircleShellMenuButton(
            common.sourceKind,
            HubSourceKind.entries.map { HarvestCircleMenuOption(it, it.name) },
            { kind ->
                presenter.edit { it.copy(common = it.common.copy(sourceKind = kind)) }
            },
            common.sourceKind.name,
            Modifier.testTag("custody-source-kind"),
        )
        ReferenceChoice(
            "Independent source issuer",
            "custody-issuer",
            draft.context.references,
            HubReferenceKind.Party,
            common.issuerId,
            { id -> presenter.edit { it.copy(common = it.common.copy(issuerId = id)) } },
        )
        CustodyField("Independent source code", "custody-source-code", common.sourceCode) { value ->
            presenter.edit {
                it.copy(common = it.common.copy(sourceCode = value))
            }
        }
        CustodyField("Source identification provenance", "custody-source-provenance", common.sourceProvenance) { value ->
            presenter.edit {
                it.copy(common = it.common.copy(sourceProvenance = value))
            }
        }
    }
}

@Composable
private fun CustodyAssociation(
    draft: CustodyDraft,
    presenter: CustodyPresenter,
) {
    val fields = draft.fields
    HarvestCircleShellText("Optional original departure association")
    CustodyField(
        "Original departure operation ID (read only)",
        "custody-departure-operation",
        fields.departureOperation,
    ) { value ->
        presenter.edit {
            it.copy(departureOperation = value)
        }
    }
    HarvestCircleShellButton("Read original departure", presenter::readDeparture, Modifier.testTag("custody-read-departure"))
    CustodyField(
        "Claimed departure record ID (blank means unknown)",
        "custody-departure-id",
        fields.departureId,
    ) { value ->
        presenter.edit {
            it.copy(departureId = value)
        }
    }
    CustodyField(
        "Claimed original group ID (blank means unknown)",
        "custody-original-group",
        fields.originalGroupId,
    ) { value ->
        presenter.edit {
            it.copy(originalGroupId = value)
        }
    }
    draft.departure?.let { old ->
        HarvestCircleShellText("Original recipient: ${old.paper.versions.last().material.recipient.value.label}")
        HarvestCircleShellText(
            "These are original departure lines. Selecting one is an explicit claim; native admission checks its current association totals.",
        )
        old.paper.versions.last().material.lines.forEach { line ->
            HarvestCircleShellPanel {
                HarvestCircleShellText(line.identification?.label ?: "Original group without separate identifier")
                line.identification?.let { HarvestCircleShellText(it.observation) }
                HarvestCircleShellText("${line.product.value.label} · ${line.variant.value.label} · ${line.location.value.label}")
                HarvestCircleShellButton(
                    "Associate with ${line.wholePacks} original departed packs · ${line.groupId}",
                    { presenter.edit { it.copy(originalGroupId = line.groupId) } },
                    Modifier.testTag("custody-associate-${line.groupId}"),
                )
            }
        }
    }
    HarvestCircleShellText(
        "A known departure does not choose an original group automatically. Unknown, absent, contradictory or overclaimed associations remain explicit; physical returned custody is recorded independently.",
    )
}
