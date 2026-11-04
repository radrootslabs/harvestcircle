package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.ActualLookupKind
import org.harvestcircle.application.ActualPresenter
import org.harvestcircle.application.ActualSubmission
import org.harvestcircle.application.ActualTarget
import org.harvestcircle.application.ActualViewState
import org.harvestcircle.application.HubActualReportFilter
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMenuButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
fun ActualWorkspace(
    state: ActualViewState,
    presenter: ActualPresenter,
    submit: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(
                rememberScrollState(),
            ).padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("actual-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleShellText("Sample data only · Conflicting actual reports and separate historical review")
        state.message?.let { HarvestCircleShellText(it, Modifier.testTag("actual-message")) }
        if (state.loading) HarvestCircleShellText("Reading native actual records…")
        val submission = state.submission
        val lookup = state.originalLookup
        val draft = state.draft
        when {
            submission != null -> {
                SelectionContainer {
                    HarvestCircleShellText("Original operation ${submission.attempt.operation.value}", Modifier.testTag("actual-operation"))
                }
                when (submission) {
                    is ActualSubmission.Pending -> HarvestCircleShellText("Waiting for the original native outcome…")
                    is ActualSubmission.Recorded -> {
                        ActualResultRecord(submission.result)
                        HarvestCircleShellButton(
                            "Done; read reports and current stock",
                            presenter::finishRecorded,
                            Modifier.testTag("actual-done"),
                            primary = true,
                        )
                    }
                    is ActualSubmission.Uncertain -> {
                        HarvestCircleShellText(submission.explanation)
                        HarvestCircleShellText("Keep this operation. Do not submit the same physical facts under a new ID.")
                        HarvestCircleShellButton(
                            "Check original result",
                            presenter::checkOriginal,
                            Modifier.testTag("actual-check-original"),
                            enabled = !state.loading,
                        )
                    }
                    is ActualSubmission.Rejected -> {
                        HarvestCircleShellText(submission.problem.safeMessage)
                        HarvestCircleShellButton(
                            "Review corrected input and current context",
                            presenter::reviewRejected,
                            Modifier.testTag("actual-review-rejected"),
                            enabled = !state.loading,
                        )
                        HarvestCircleShellButton("Check original result", presenter::checkOriginal, enabled = !state.loading)
                    }
                }
            }
            lookup != null -> {
                HarvestCircleShellMenuButton(
                    lookup.kind,
                    ActualLookupKind.entries.map {
                        HarvestCircleMenuOption(it, it.name)
                    },
                    { presenter.editOriginalLookup(it, lookup.operationText) },
                    lookup.kind.name,
                    Modifier.testTag("actual-lookup-kind"),
                )
                ActualField(
                    "Original operation ID",
                    "actual-lookup-operation",
                    lookup.operationText,
                ) { presenter.editOriginalLookup(lookup.kind, it) }
                HarvestCircleShellButton(
                    "Read original only",
                    presenter::lookupOriginal,
                    Modifier.testTag("actual-lookup-submit"),
                    enabled = !state.loading,
                )
                lookup.result?.let { ActualResultRecord(it) }
                HarvestCircleShellText("Missing history after restore does not prove no effect. No saved intent is executed here.")
                HarvestCircleShellButton("Close original lookup", presenter::discard, enabled = !state.loading)
            }
            draft != null -> {
                HarvestCircleShellButton(
                    "Back; keep unsubmitted draft",
                    presenter::park,
                    Modifier.testTag("actual-park"),
                    enabled = !state.loading,
                )
                when (val target = draft.target) {
                    ActualTarget.Report -> Unit
                    is ActualTarget.Departure -> ActualReportRecord(target.report)
                    is ActualTarget.Link -> ActualReportRecord(target.report)
                }
                HarvestCircleShellButton(
                    "Read current choices and context",
                    presenter::refreshContext,
                    Modifier.testTag("actual-refresh-context"),
                    enabled = !state.loading,
                )
                if (state.needsReview) {
                    HarvestCircleShellText(
                        "Review current native context before preview or confirmation. Input is retained.",
                    )
                }
                state.currentContext?.let { context ->
                    HarvestCircleShellPanel {
                        HarvestCircleShellText("Current stock ${context.stock?.stockRevision} · Catalog ${context.stock?.catalogRevision}")
                        context.group?.let { group ->
                            HarvestCircleShellText("Current group ${group.group.id} · Version ${group.group.revision}")
                            HarvestCircleShellText("${group.group.wholePacks} whole packs · ${group.location.value.label}")
                            HarvestCircleShellText("Current acceptance ${context.treatment?.acceptance}")
                            HarvestCircleShellText("Current hold IDs ${context.treatment?.activeHoldIds}")
                            group.group.work.forEach { work ->
                                HarvestCircleShellText("${work.progress} · ${work.requirement.quantity}")
                            }
                        }
                        HarvestCircleShellText("Current choices are separate from historical source and permission.")
                        HarvestCircleShellButton(
                            "Use reviewed current context",
                            presenter::acceptContext,
                            Modifier.testTag("actual-accept-context"),
                            enabled = !state.loading,
                        )
                    }
                }
                ActualEditor(draft, presenter)
                if (draft.target !=
                    ActualTarget.Report
                ) {
                    HarvestCircleShellButton(
                        "Read native comparison without effects",
                        presenter::preview,
                        Modifier.testTag("actual-preview"),
                        enabled =
                            !state.loading && !state.needsReview,
                    )
                }
                state.preview?.let { ActualPreviewRecord(it) }
                HarvestCircleShellButton(
                    if (draft.fields.confirmed) {
                        "Confirmation recorded; clear"
                    } else {
                        "I confirm these reported facts and displayed consequences"
                    },
                    {
                        presenter.confirm(!draft.fields.confirmed)
                    },
                    Modifier.testTag("actual-confirm"),
                    enabled =
                        !state.loading && !state.needsReview && (draft.target == ActualTarget.Report || state.preview != null),
                )
                HarvestCircleShellButton(
                    when (draft.target) {
                        ActualTarget.Report -> "Save report only"
                        is ActualTarget.Departure -> "Record reviewed physical departure"
                        is ActualTarget.Link -> "Link to known departure without another effect"
                    },
                    submit,
                    Modifier.testTag("actual-submit"),
                    primary = true,
                    enabled =
                        !state.loading && !state.needsReview && draft.fields.confirmed,
                )
                HarvestCircleShellButton("Discard unsubmitted input", presenter::discard, enabled = !state.loading)
            }
            else -> {
                HarvestCircleShellButton("Back to operations", presenter::park, Modifier.testTag("actual-park"))
                HarvestCircleShellButton(
                    "Report conflicting actual facts",
                    presenter::beginReport,
                    Modifier.testTag("actual-new-report"),
                    enabled = !state.loading,
                )
                HarvestCircleShellButton(
                    "Read original by operation",
                    presenter::beginOriginalLookup,
                    Modifier.testTag("actual-open-lookup"),
                    enabled = !state.loading,
                )
                HarvestCircleShellMenuButton(
                    state.filter,
                    HubActualReportFilter.entries.map {
                        HarvestCircleMenuOption(it, it.name)
                    },
                    { presenter.refreshReports(it) },
                    state.filter.name,
                    Modifier.testTag("actual-filter"),
                )
                HarvestCircleShellText(
                    "All reports includes reviewed findings. No unresolved reports does not mean breaches or current holds were cleared.",
                )
                state.page?.let { page ->
                    if (page.rows.isEmpty()) {
                        HarvestCircleShellText(
                            "No reports in this page and filter. Historical and current treatment are separate.",
                        )
                    }
                    page.rows.forEach { row ->
                        ActualReportRecord(row.report)
                        val review = row.reviewOperationId
                        if (review == null) {
                            HarvestCircleShellButton("Review a separate physical departure", {
                                presenter.beginDeparture(row.report)
                            }, Modifier.testTag("actual-review-${row.report.id}"), enabled = !state.loading)
                            HarvestCircleShellButton("Compare with known departure", {
                                presenter.beginLink(row.report)
                            }, Modifier.testTag("actual-link-${row.report.id}"), enabled = !state.loading)
                        } else if (row.resolutionKind == org.harvestcircle.application.HubActualReportResolutionKind.AdditionalQuantity) {
                            HarvestCircleShellText(
                                "Additional quantity correction ${review.value}; raw facts and original departure remain retained. See trace history for the linked correction.",
                            )
                        } else {
                            HarvestCircleShellText("Reviewed original ${review.value}; findings remain retained.")
                            HarvestCircleShellButton("Read retained review", {
                                presenter.beginOriginalLookup()
                                presenter.editOriginalLookup(ActualLookupKind.Review, review.value)
                                presenter.lookupOriginal()
                            }, Modifier.testTag("actual-original-${row.report.id}"), enabled = !state.loading)
                        }
                    }
                    HarvestCircleShellButton("First reports", { presenter.refreshReports() }, enabled = !state.loading)
                    HarvestCircleShellButton(
                        "Next reports",
                        { presenter.refreshReports(next = true) },
                        Modifier.testTag("actual-next"),
                        enabled =
                            !state.loading && page.more,
                    )
                }
            }
        }
    }
}
