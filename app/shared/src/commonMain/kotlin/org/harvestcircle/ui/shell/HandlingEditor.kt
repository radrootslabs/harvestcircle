package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HandlingDraft
import org.harvestcircle.application.HandlingFields
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HandlingViewState
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.MovementScope
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
internal fun HandlingEditor(
    state: HandlingViewState,
    presenter: HandlingPresenter,
    submit: () -> Unit,
) {
    val draft = requireNotNull(state.draft)
    val fields = draft.fields

    fun edit(change: (HandlingFields) -> HandlingFields) = presenter.edit(change(fields))
    HarvestCircleShellText(handlingTitle(draft), role = HarvestCircleShellTextRole.SectionTitle)
    HandlingDraftScope(draft)
    when (draft) {
        is HandlingDraft.Move -> {
            HarvestCircleShellText("Record actual arrival. A planned destination alone never moves stock.")
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                HarvestCircleShellButton(
                    "Whole selected group",
                    { edit { it.copy(scope = MovementScope.WholeGroup) } },
                    Modifier.testTag("handling-whole-group"),
                    primary = fields.scope == MovementScope.WholeGroup,
                )
                HarvestCircleShellButton(
                    "Identified portion",
                    { edit { it.copy(scope = MovementScope.IdentifiedPortion) } },
                    Modifier.testTag("handling-portion"),
                    primary = fields.scope == MovementScope.IdentifiedPortion,
                )
            }
            if (fields.scope == MovementScope.IdentifiedPortion) PhysicalPortionFields(fields, presenter)
            HandlingOccurrence(fields, presenter)
        }
        is HandlingDraft.Split -> {
            HarvestCircleShellText(
                "Identify a separately distinguishable portion at this location. Count and handling progress stay unchanged.",
            )
            PhysicalPortionFields(fields, presenter)
            HandlingOccurrence(fields, presenter)
        }
        is HandlingDraft.Replan ->
            HarvestCircleShellText(
                "Retire the remaining work and create a distinct requirement at the new target. Completed work and stock stay unchanged.",
            )
        is HandlingDraft.NewWork ->
            HarvestCircleShellText(
                "Create explicit new handling for this selected group. Existing pending work must be completed or replanned first. This does not move stock.",
            )
        is HandlingDraft.OpenDiscrepancy ->
            HarvestCircleShellText(
                "Record the observation for follow-up. This does not place a hold or change physical stock.",
            )
        is HandlingDraft.ResolveDiscrepancy ->
            HarvestCircleShellText(
                "Retain the original observation and record your resolution. Independent holds and handling work remain unchanged.",
            )
    }
    if (draft is HandlingDraft.Move || draft is HandlingDraft.Replan || draft is HandlingDraft.NewWork) {
        ReferenceChoice(
            "Destination",
            "handling-target",
            state.references,
            HubReferenceKind.Location,
            fields.targetId,
            { edit { current -> current.copy(targetId = it) } },
            optional = false,
            emptyLabel = "Choose destination",
        )
    }
    HarvestCircleShellTextField(
        fields.observation,
        { edit { current -> current.copy(observation = it) } },
        if (draft is HandlingDraft.ResolveDiscrepancy) "Resolution and basis" else "Observation and basis",
        "Manually confirmed facts",
        inputModifier = Modifier.testTag("handling-observation"),
    )
    if (state.needsReview) HarvestCircleShellText("The selected context changed. Review the current scope before another submission.")
    HarvestCircleShellButton(
        "Review current scope",
        presenter::reviewContext,
        Modifier.testTag("handling-review-context"),
        enabled = !state.loading,
    )
    state.review?.let { review ->
        HarvestCircleShellPanel(Modifier.testTag("handling-context-review")) {
            HarvestCircleShellText("Current scope", role = HarvestCircleShellTextRole.BodyStrong)
            HandlingDraftScope(review.draft)
            HarvestCircleShellText(
                "Your observation, physical identifier and selected destination remain retained. Confirm the current scope before continuing.",
            )
            HarvestCircleShellButton(
                "Use reviewed scope",
                presenter::acceptReview,
                Modifier.testTag("handling-accept-review"),
                enabled = !state.loading,
            )
        }
    }
    HarvestCircleShellButton(
        "Record",
        submit,
        Modifier.testTag("handling-save"),
        primary = true,
        enabled =
            !state.loading && !state.needsReview,
    )
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton("Keep draft and close", presenter::parkDraft, Modifier.testTag("handling-park"), enabled = !state.loading)
        HarvestCircleShellButton(
            "Discard unsubmitted draft",
            presenter::discardDraft,
            Modifier.testTag("handling-discard"),
            enabled = !state.loading,
        )
    }
}

@Composable
internal fun HandlingDraftScope(draft: HandlingDraft) {
    when (draft) {
        is HandlingDraft.Split ->
            HarvestCircleShellText(
                "Selected: ${draft.selected.group.wholePacks} original packs at ${draft.selected.location.value.label}",
            )
        is HandlingDraft.Move ->
            HarvestCircleShellText(
                "Selected: ${draft.selected.group.wholePacks} original packs at ${draft.selected.location.value.label}",
            )
        is HandlingDraft.NewWork ->
            HarvestCircleShellText(
                "Selected: ${draft.selected.group.wholePacks} original packs at ${draft.selected.location.value.label}",
            )
        is HandlingDraft.OpenDiscrepancy ->
            HarvestCircleShellText(
                "Selected: ${draft.selected.group.wholePacks} original packs at ${draft.selected.location.value.label}",
            )
        is HandlingDraft.Replan -> HandlingQuantitySummary(draft.selected.work.quantity)
        is HandlingDraft.ResolveDiscrepancy -> {
            HarvestCircleShellText("Original observation: ${draft.selected.value.observation}")
            draft.selected.value.resolution
                ?.let { HarvestCircleShellText("Already resolved: $it") }
        }
    }
}

@Composable
private fun HandlingOccurrence(
    fields: HandlingFields,
    presenter: HandlingPresenter,
) {
    HarvestCircleShellText("When did this handling actually occur?")
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HubOccurrenceKind.entries.forEach { kind ->
            HarvestCircleShellButton(
                kind.name,
                { presenter.edit(fields.copy(occurrenceKind = kind)) },
                Modifier.testTag("handling-time-$kind"),
                primary = fields.occurrenceKind == kind,
            )
        }
    }
    if (fields.occurrenceKind != HubOccurrenceKind.Unknown) {
        HarvestCircleShellTextField(
            fields.occurrenceStart,
            { presenter.edit(fields.copy(occurrenceStart = it)) },
            if (fields.occurrenceKind == HubOccurrenceKind.Interval) "Earliest occurrence" else "Actual occurrence",
            "ISO instant or timestamp with offset",
            inputModifier = Modifier.testTag("handling-time-start"),
        )
    }
    if (fields.occurrenceKind == HubOccurrenceKind.Interval) {
        HarvestCircleShellTextField(
            fields.occurrenceEnd,
            { presenter.edit(fields.copy(occurrenceEnd = it)) },
            "Latest occurrence",
            "ISO instant or timestamp with offset",
            inputModifier = Modifier.testTag("handling-time-end"),
        )
    }
}

internal fun handlingTitle(draft: HandlingDraft) =
    when (draft) {
        is HandlingDraft.Split -> "Separate identified portion"
        is HandlingDraft.Move -> "Record actual movement"
        is HandlingDraft.Replan -> "Replan remaining work"
        is HandlingDraft.NewWork -> "New handling requirement"
        is HandlingDraft.OpenDiscrepancy -> "Report discrepancy"
        is HandlingDraft.ResolveDiscrepancy -> "Resolve discrepancy follow-up"
    }

@Composable
private fun PhysicalPortionFields(
    fields: HandlingFields,
    presenter: HandlingPresenter,
) {
    fun edit(change: (HandlingFields) -> HandlingFields) = presenter.edit(change(fields))
    HarvestCircleShellText("The portion must be physically distinguishable, for example by a tag or separate container.")
    HarvestCircleShellTextField(
        fields.quantity,
        {
            edit { current ->
                current.copy(quantity = it)
            }
        },
        "Original whole packs",
        "Count",
        inputModifier = Modifier.testTag("handling-quantity"),
    )
    HarvestCircleShellTextField(
        fields.physicalLabel,
        {
            edit { current ->
                current.copy(physicalLabel = it)
            }
        },
        "Physical identifier",
        "Tag or separate container",
        inputModifier = Modifier.testTag("handling-physical-label"),
    )
    HarvestCircleShellTextField(
        fields.physicalObservation,
        {
            edit { current ->
                current.copy(physicalObservation = it)
            }
        },
        "How this portion is distinguishable",
        "Observed separation",
        inputModifier = Modifier.testTag("handling-physical-observation"),
    )
}
