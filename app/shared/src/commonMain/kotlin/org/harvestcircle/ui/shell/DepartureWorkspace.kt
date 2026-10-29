package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.DeparturePresenter
import org.harvestcircle.application.DepartureSubmission
import org.harvestcircle.application.DepartureViewState
import org.harvestcircle.application.HubDepartureWorkChange
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellCheckbox
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
internal fun DepartureWorkspace(
    state: DepartureViewState,
    presenter: DeparturePresenter,
    submit: () -> Unit,
    finish: () -> Unit,
    discard: () -> Unit,
) {
    val scroll = rememberScrollState()
    LaunchedEffect(state.submission) { scroll.scrollTo(0) }
    Column(
        Modifier.fillMaxSize().padding(HarvestCircleTheme.shell.layout.contentGap).testTag("departure-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        Column(
            Modifier.weight(1f).verticalScroll(scroll),
            verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
        ) {
            HarvestCircleShellText(
                "Sample data only · Record packs that have actually left",
                role = HarvestCircleShellTextRole.SectionTitle,
            )
            state.message?.let { HarvestCircleShellText(it, Modifier.testTag("departure-message")) }
            if (state.loading) HarvestCircleShellText("Reviewing current departure context…")
            if (state.submission != null) {
                DepartureOutcome(state, presenter, finish)
            } else {
                DepartureEditor(state, presenter)
            }
        }
        if (state.submission == null) DepartureActions(state, presenter, submit, discard)
    }
}

@Composable
private fun DepartureEditor(
    state: DepartureViewState,
    presenter: DeparturePresenter,
) {
    val draft = requireNotNull(state.draft)
    val version = draft.paper.versions.last()
    HarvestCircleShellPanel {
        HarvestCircleShellText("As-issued identification · Version ${version.number}", role = HarvestCircleShellTextRole.BodyStrong)
        DispatchMaterial(version.material)
        HarvestCircleShellText("Identification: ${draft.request.identificationId}")
        HarvestCircleShellText(
            "Recipient and quantities stay as issued. A smaller departure requires a recorded physical split and replacement identification.",
        )
    }
    HarvestCircleShellTextField(
        draft.fields.observation,
        { presenter.edit(draft.fields.copy(observation = it)) },
        "Actual departure observation",
        "Describe what left and the evidence observed",
        inputModifier = Modifier.testTag("departure-observation"),
    )
    HarvestCircleShellText("When did these packs actually leave? Recording time and operator are captured separately.")
    HubOccurrenceKind.entries.forEach { kind ->
        HarvestCircleShellButton(
            when (kind) {
                HubOccurrenceKind.Unknown -> "Actual time unknown"
                HubOccurrenceKind.Known -> "Known actual time"
                HubOccurrenceKind.Interval -> "Actual time interval"
            },
            { presenter.edit(draft.fields.copy(occurrenceKind = kind)) },
            Modifier.testTag("departure-time-$kind"),
            primary = draft.fields.occurrenceKind == kind,
        )
    }
    if (draft.fields.occurrenceKind != HubOccurrenceKind.Unknown) {
        HarvestCircleShellTextField(
            draft.fields.occurrenceStart,
            { presenter.edit(draft.fields.copy(occurrenceStart = it)) },
            "Actual time (UTC or explicit offset)",
            "2026-10-29T10:00:00Z",
            inputModifier = Modifier.testTag("departure-time-start"),
        )
    }
    if (draft.fields.occurrenceKind == HubOccurrenceKind.Interval) {
        HarvestCircleShellTextField(
            draft.fields.occurrenceEnd,
            { presenter.edit(draft.fields.copy(occurrenceEnd = it)) },
            "Interval end (UTC or explicit offset)",
            "2026-10-29T10:15:00Z",
            inputModifier = Modifier.testTag("departure-time-end"),
        )
    }
    val context = state.review ?: draft.context
    context?.let {
        HarvestCircleShellPanel {
            HarvestCircleShellText(if (it.ready) "Current native checks allow this departure" else "Departure needs attention")
            it.block?.let { problem -> HarvestCircleShellText(problem.safeMessage, Modifier.testTag("departure-block")) }
            HarvestCircleShellText(
                "Current checks do not reserve stock. Material, holds, acceptance, authority and actual time are checked again when recording.",
            )
            DepartureWorkChanges(it.proposedWorkChanges, state.workOffset, presenter, proposed = true)
        }
    }
    if (state.needsReview) HarvestCircleShellText("Review the same issued identification against current context before confirming again.")
    HarvestCircleShellButton(
        "Check current context",
        presenter::reviewContext,
        Modifier.testTag("departure-review-context"),
        enabled = !state.loading,
    )
    if (state.review != null) {
        HarvestCircleShellButton(
            "Accept reviewed context",
            presenter::acceptReview,
            Modifier.testTag("departure-accept-review"),
            enabled =
                !state.loading && state.review.ready,
        )
    }
}

@Composable
private fun DepartureActions(
    state: DepartureViewState,
    presenter: DeparturePresenter,
    submit: () -> Unit,
    discard: () -> Unit,
) {
    val draft = requireNotNull(state.draft)
    val ready = !state.loading && !state.needsReview && draft.context?.ready == true
    HarvestCircleShellCheckbox(
        "I confirm all listed packs have actually left for the stated recipient",
        draft.confirmed,
        presenter::confirm,
        Modifier.testTag("departure-confirm"),
        enabled = ready,
    )
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton(
            "Record actual departure",
            submit,
            Modifier.testTag("departure-save"),
            primary = true,
            enabled = ready && draft.confirmed && draft.fields.observation.isNotBlank(),
        )
        HarvestCircleShellButton("Back; keep draft", presenter::park, Modifier.testTag("departure-keep"), enabled = !state.loading)
        HarvestCircleShellButton("Discard unsubmitted draft", discard, Modifier.testTag("departure-discard"), enabled = !state.loading)
    }
}

@Composable
private fun DepartureOutcome(
    state: DepartureViewState,
    presenter: DeparturePresenter,
    finish: () -> Unit,
) {
    val result = requireNotNull(state.submission)
    SelectionContainer {
        HarvestCircleShellText("Original operation: ${result.attempt.operation.value}", Modifier.testTag("departure-operation"))
    }
    when (result) {
        is DepartureSubmission.Pending -> HarvestCircleShellText("Recording actual departure…", Modifier.testTag("departure-pending"))
        is DepartureSubmission.Recorded -> {
            val receipt = result.receipt
            HarvestCircleShellText(
                "Actual departure recorded",
                Modifier.testTag("departure-recorded"),
                HarvestCircleShellTextRole.SectionTitle,
            )
            HarvestCircleShellText("Recorded: ${stockUtc(receipt.recordedAt.value)} · Operator ${receipt.actor.value}")
            HarvestCircleShellText("Observation: ${receipt.observation}")
            HarvestCircleShellText("Actual occurrence: ${receipt.occurrence.kind}")
            receipt.occurrence.startUnixSeconds?.let { HarvestCircleShellText("Start: ${stockUtc(it)}") }
            receipt.occurrence.endUnixSeconds?.let { HarvestCircleShellText("End: ${stockUtc(it)}") }
            receipt.paper.versions
                .lastOrNull()
                ?.let { DispatchMaterial(it.material) }
            DepartureWorkChanges(receipt.workChanges, state.workOffset, presenter, proposed = false)
            HarvestCircleShellButton("View used identification", finish, Modifier.testTag("departure-done"), primary = true)
        }
        is DepartureSubmission.Rejected -> {
            HarvestCircleShellText(result.problem.safeMessage, Modifier.testTag("departure-rejected"))
            HarvestCircleShellText(
                "The native command was rejected. Retain the observation and review the same issued identification before confirming again.",
            )
            HarvestCircleShellButton(
                "Review retained input",
                presenter::reviewRejected,
                Modifier.testTag("departure-review-rejected"),
                enabled = !state.loading,
            )
            DepartureLookup(state, presenter)
        }
        is DepartureSubmission.Uncertain -> {
            HarvestCircleShellText(result.message, Modifier.testTag("departure-uncertain"))
            HarvestCircleShellText(
                "Checking the original result is read-only. Do not record this departure again with a different operation.",
            )
            DepartureLookup(state, presenter)
        }
    }
}

@Composable
private fun DepartureLookup(
    state: DepartureViewState,
    presenter: DeparturePresenter,
) {
    HarvestCircleShellButton(
        "Check original result",
        presenter::checkOriginal,
        Modifier.testTag("departure-check-original"),
        enabled = !state.loading,
    )
}

@Composable
private fun DepartureWorkChanges(
    changes: List<HubDepartureWorkChange>,
    offset: Int,
    presenter: DeparturePresenter,
    proposed: Boolean,
) {
    HarvestCircleShellText(if (proposed) "Proposed work changes" else "Recorded work changes", role = HarvestCircleShellTextRole.BodyStrong)
    if (changes.isEmpty()) HarvestCircleShellText("No pending work is retired by this departure.")
    changes.drop(offset).take(64).forEach { change ->
        HarvestCircleShellPanel {
            HarvestCircleShellText("Requirement ${change.before.id}")
            HarvestCircleShellText(
                "${change.retiredWholePacks} pending packs become no longer required. Historical completion is retained.",
            )
            HarvestCircleShellText(
                "Before: completed ${change.before.quantity.completed} · No longer required ${change.before.quantity.noLongerRequired} · Remaining ${change.before.quantity.remaining}",
            )
            HandlingQuantitySummary(change.after.quantity)
        }
    }
    if (changes.size > 64) {
        HarvestCircleShellText("Work changes ${offset + 1}–${minOf(offset + 64, changes.size)} of ${changes.size}")
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellButton("Previous work changes", { presenter.workPage(-1) }, enabled = offset > 0)
            HarvestCircleShellButton("Next work changes", { presenter.workPage(1) }, enabled = offset + 64 < changes.size)
        }
    }
}
