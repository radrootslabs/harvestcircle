package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.ActualChoicePage
import org.harvestcircle.application.ActualDraft
import org.harvestcircle.application.ActualPresenter
import org.harvestcircle.application.ActualTarget
import org.harvestcircle.application.HubActualSourceKind
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMenuButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField

@Composable
internal fun ActualEditor(
    draft: ActualDraft,
    presenter: ActualPresenter,
) {
    val fields = draft.fields
    if (draft.target is ActualTarget.Link) {
        HarvestCircleShellText("Choose a population including depleted stock; linking never creates another reduction.")
        draft.context.traces?.rows?.forEach { row ->
            HarvestCircleShellButton(
                row.trace.id,
                { presenter.edit { it.copy(traceId = row.trace.id) } },
                Modifier.testTag("actual-trace-${row.trace.id}"),
                primary =
                    fields.traceId == row.trace.id,
            )
        }
        if (draft.context.traces?.more ==
            true
        ) {
            HarvestCircleShellButton("Next populations", { presenter.nextChoices(ActualChoicePage.Traces) })
        }
        HarvestCircleShellText("After selecting a population, read and use current context to load immutable departure choices.")
        draft.context.known.forEach { known ->
            HarvestCircleShellButton(
                known.label,
                { presenter.edit { it.copy(known = known) } },
                Modifier.testTag("actual-known-${known.key}"),
                primary =
                    fields.known == known,
            )
        }
        ActualField(
            "Verification that this report describes the same complete known departure",
            "actual-duplicate-basis",
            fields.duplicateBasis,
        ) { value ->
            presenter.edit {
                it.copy(duplicateBasis = value)
            }
        }
        return
    }
    if (draft.target == ActualTarget.Report) {
        HarvestCircleShellText("Save unverified facts even when quantity, time, recipient or source is unknown. No physical adjustment.")
        ReferenceChoice(
            "Reported pack (optional)",
            "actual-variant",
            draft.context.references,
            HubReferenceKind.Pack,
            fields.variantId,
            { id ->
                presenter
                    .edit {
                        it.copy(variantId = id)
                    }
            },
        )
        HarvestCircleShellMenuButton(
            fields.sourceKind,
            HubActualSourceKind.entries.map {
                HarvestCircleMenuOption(
                    it,
                    if (it ==
                        HubActualSourceKind.Reported
                    ) {
                        "Reporter claims one source"
                    } else {
                        it.name
                    },
                )
            },
            { kind ->
                presenter.edit { it.copy(sourceKind = kind, sourceTraceIds = emptyList()) }
            },
            fields.sourceKind.name,
            Modifier.testTag("actual-source-kind"),
        )
        if (fields.sourceKind != HubActualSourceKind.Unknown) {
            HarvestCircleShellText("Claims are population references only. No historical group or location is invented.")
            draft.context.traces?.rows?.forEach { row ->
                val id = row.trace.id
                HarvestCircleShellButton("${row.trace.product.value.label} · $id", {
                    presenter.edit {
                        it.copy(
                            sourceTraceIds =
                                if (it.sourceKind ==
                                    HubActualSourceKind.Reported
                                ) {
                                    listOf(id)
                                } else if (id in
                                    it.sourceTraceIds
                                ) {
                                    it.sourceTraceIds - id
                                } else {
                                    it.sourceTraceIds + id
                                },
                        )
                    }
                }, Modifier.testTag("actual-source-$id"), primary = id in fields.sourceTraceIds)
            }
            HarvestCircleShellText("Selected claimed populations: ${fields.sourceTraceIds.joinToString().ifEmpty { "None" }}")
            if (draft.context.traces?.more ==
                true
            ) {
                HarvestCircleShellButton("Next claimed populations", { presenter.nextChoices(ActualChoicePage.Traces) })
            }
        }
    } else {
        HarvestCircleShellText(
            "Review a physical departure separately from planned permission. Choose balancing stock and a historical birth or arrival.",
        )
        draft.context.stock?.rows?.forEach { row ->
            HarvestCircleShellButton(
                "${row.location.value.label} · ${row.position.wholePacks} packs · ${row.position.groupId}",
                {
                    presenter.edit { it.copy(groupId = row.position.groupId) }
                },
                Modifier.testTag("actual-group-${row.position.groupId}"),
                primary =
                    fields.groupId == row.position.groupId,
            )
        }
        if (draft.context.stock?.more ==
            true
        ) {
            HarvestCircleShellButton("Next stock groups", { presenter.nextChoices(ActualChoicePage.Stock) })
        }
        HarvestCircleShellText("After choosing a balancing group, read and use current context before choosing historical evidence.")
        draft.context.group?.let {
            HarvestCircleShellText(
                "Group ${it.group.id} · Version ${it.group.revision} · ${it.group.wholePacks} packs",
            )
            HarvestCircleShellText(
                "Current acceptance ${draft.context.treatment?.acceptance} · Current holds ${draft.context.treatment?.activeHoldIds}",
            )
        }
        draft.context.sources.forEach { source ->
            HarvestCircleShellButton(
                source.label,
                { presenter.edit { it.copy(source = source) } },
                Modifier.testTag("actual-birth-${source.key}"),
                primary =
                    fields.source == source,
            )
        }
        if (draft.context.lineage?.more ==
            true
        ) {
            HarvestCircleShellButton("Next births", { presenter.nextChoices(ActualChoicePage.Lineage) })
        }
        if (draft.context.sourceHistory?.more ==
            true
        ) {
            HarvestCircleShellButton("Next historical arrivals", { presenter.nextChoices(ActualChoicePage.History) })
        }
        HarvestCircleShellText(
            "Selected historical source: ${fields.source?.label ?: "Not selected"}. Native preview checks the complete history.",
        )
        ActualField("Evidence for historical source", "actual-source-evidence", fields.sourceEvidence) { value ->
            presenter.edit {
                it.copy(sourceEvidence = value)
            }
        }
        ActualField("Evidence for quantity", "actual-quantity-evidence", fields.quantityEvidence) { value ->
            presenter.edit {
                it.copy(quantityEvidence = value)
            }
        }
        ActualField("Evidence for recipient", "actual-recipient-evidence", fields.recipientEvidence) { value ->
            presenter.edit {
                it.copy(recipientEvidence = value)
            }
        }
        ActualField("Evidence for occurrence", "actual-timing-evidence", fields.timingEvidence) { value ->
            presenter.edit {
                it.copy(timingEvidence = value)
            }
        }
        ActualField(
            "Comparison against known departures; why this is a separate physical event",
            "actual-duplicate-basis",
            fields.duplicateBasis,
        ) { value ->
            presenter.edit {
                it.copy(duplicateBasis = value)
            }
        }
    }
    ActualField(
        if (draft.target ==
            ActualTarget.Report
        ) {
            "Reported whole packs (blank means unknown)"
        } else {
            "Reviewed whole packs"
        },
        "actual-quantity",
        fields.quantity,
    ) { value -> presenter.edit { it.copy(quantity = value) } }
    ReferenceChoice(
        "Recipient",
        "actual-recipient",
        draft.context.references,
        HubReferenceKind.Party,
        fields.recipientId,
        { id ->
            presenter.edit {
                it.copy(recipientId = id)
            }
        },
        optional =
            draft.target == ActualTarget.Report,
    )
    HarvestCircleShellMenuButton(
        fields.occurrenceKind,
        HubOccurrenceKind.entries.map {
            HarvestCircleMenuOption(it, it.name)
        },
        { kind ->
            presenter.edit {
                it.copy(
                    occurrenceKind = kind,
                )
            }
        },
        fields.occurrenceKind.name,
        Modifier.testTag("actual-occurrence-kind"),
    )
    if (fields.occurrenceKind !=
        HubOccurrenceKind.Unknown
    ) {
        ActualField("Occurrence UTC (ISO 8601)", "actual-occurrence-start", fields.occurrenceStart) { value ->
            presenter.edit {
                it.copy(occurrenceStart = value)
            }
        }
    }
    if (fields.occurrenceKind ==
        HubOccurrenceKind.Interval
    ) {
        ActualField("Latest occurrence UTC (ISO 8601)", "actual-occurrence-end", fields.occurrenceEnd) { value ->
            presenter.edit {
                it.copy(occurrenceEnd = value)
            }
        }
    }
    HarvestCircleShellText("Recording time is separate. Unknown time stays unknown; a future report claim is retained as a clock conflict.")
    if (draft.target ==
        ActualTarget.Report
    ) {
        ActualField("What was reported and by whom", "actual-observation", fields.observation) { value ->
            presenter.edit {
                it.copy(observation = value)
            }
        }
    }
}

@Composable
internal fun ActualField(
    label: String,
    tag: String,
    value: String,
    change: (String) -> Unit,
) {
    HarvestCircleShellTextField(value, change, label, label, inputModifier = Modifier.testTag(tag))
}
