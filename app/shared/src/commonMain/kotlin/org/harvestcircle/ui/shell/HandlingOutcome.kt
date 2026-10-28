package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HandlingSubmission
import org.harvestcircle.application.HandlingViewState
import org.harvestcircle.application.HubHandlingResult
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole

@Composable
internal fun HandlingOutcome(
    state: HandlingViewState,
    presenter: HandlingPresenter,
    finish: () -> Unit,
) {
    val submitted = requireNotNull(state.submission)
    HarvestCircleShellPanel(Modifier.testTag("handling-outcome")) {
        HarvestCircleShellText("Original operation: ${submitted.attempt.operation.value}")
        when (submitted) {
            is HandlingSubmission.Pending -> HarvestCircleShellText("Recording the submitted observation…")
            is HandlingSubmission.Recorded -> {
                HarvestCircleShellText("Handling recorded", Modifier.testTag("handling-recorded"), HarvestCircleShellTextRole.SectionTitle)
                HandlingResultSummary(submitted.receipt.result)
                HarvestCircleShellText("Recorded: ${stockUtc(submitted.receipt.recordedAt.value)} · ${submitted.receipt.actor.value}")
                HarvestCircleShellButton("View current records", finish, Modifier.testTag("handling-done"), primary = true)
            }
            is HandlingSubmission.Rejected -> {
                HarvestCircleShellText(submitted.problem.safeMessage, Modifier.testTag("handling-rejected"))
                HarvestCircleShellButton(
                    "Review retained input",
                    presenter::reviewRejected,
                    Modifier.testTag("handling-review-rejected"),
                    enabled = !state.loading,
                )
                HarvestCircleShellButton(
                    "Check original result",
                    presenter::checkOriginal,
                    Modifier.testTag("handling-check-original"),
                    enabled = !state.loading,
                )
            }
            is HandlingSubmission.Uncertain -> {
                HarvestCircleShellText(submitted.message, Modifier.testTag("handling-uncertain"))
                HarvestCircleShellText(
                    "Checking reads the original result. It does not record another movement or create another operation.",
                )
                HarvestCircleShellButton(
                    "Check original result",
                    presenter::checkOriginal,
                    Modifier.testTag("handling-check-original"),
                    enabled = !state.loading,
                )
            }
        }
    }
}

@Composable
private fun HandlingResultSummary(result: HubHandlingResult) {
    when (result) {
        is HubHandlingResult.Moved -> {
            HarvestCircleShellText("${result.wholePacks} original packs · ${result.from.value.label} → ${result.to.value.label}")
            HarvestCircleShellText("Left at the prior location from this selection: ${result.remainingWholePacks}")
            result.separatedPortion?.let { HarvestCircleShellText("Physical identifier: ${it.label} · ${it.observation}") }
            HarvestCircleShellText("Actual occurrence: ${result.occurrence.kind}")
            result.occurrence.startUnixSeconds?.let { HarvestCircleShellText("Occurrence start: ${stockUtc(it)}") }
            result.occurrence.endUnixSeconds?.let { HarvestCircleShellText("Occurrence end: ${stockUtc(it)}") }
            HarvestCircleShellText("Observation: ${result.observation}")
            result.workChanges.forEach { HandlingQuantitySummary(it.after.quantity) }
        }
        is HubHandlingResult.WorkChanged -> {
            HarvestCircleShellText("New requirement", role = HarvestCircleShellTextRole.BodyStrong)
            HandlingQuantitySummary(result.replacement.quantity)
            result.previous?.let {
                HarvestCircleShellText("Previous requirement retained", role = HarvestCircleShellTextRole.BodyStrong)
                HandlingQuantitySummary(it.after.quantity)
            }
            HarvestCircleShellText("Basis: ${result.observation}")
            HarvestCircleShellText("Physical stock was not changed by this work plan.")
        }
        is HubHandlingResult.DiscrepancyChanged -> {
            HarvestCircleShellText("Original observation: ${result.value.observation}")
            result.value.resolution?.let { HarvestCircleShellText("Resolution: $it") }
            HarvestCircleShellText("Stock, handling work and independent holds were not changed by this follow-up.")
        }
    }
}
