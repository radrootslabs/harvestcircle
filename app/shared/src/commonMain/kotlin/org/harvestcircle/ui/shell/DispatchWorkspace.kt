package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.DispatchPresenter
import org.harvestcircle.application.DispatchSubmission
import org.harvestcircle.application.DispatchViewState
import org.harvestcircle.application.HubDispatchFilter
import org.harvestcircle.application.HubDispatchKind
import org.harvestcircle.application.HubDispatchState
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
fun DispatchWorkspace(
    state: DispatchViewState,
    presenter: DispatchPresenter,
    submit: () -> Unit,
    references: () -> Unit,
) {
    val departure by presenter.departure.state.collectAsState()
    if (departure.formOpen || departure.submission != null) {
        DepartureWorkspace(departure, presenter.departure, submit, presenter::finishDeparture, presenter::discardDeparture)
        return
    }
    val scroll = rememberScrollState()
    LaunchedEffect(state.formOpen, state.submission, state.selected?.record?.id) { scroll.scrollTo(0) }
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(scroll)
            .padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("dispatch-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleShellText("Sample data only · Identification reserves no stock")
        state.message?.let { HarvestCircleShellText(it, Modifier.testTag("dispatch-message")) }
        if (state.loading) HarvestCircleShellText("Reading local identification…")
        if (departure.draft != null) {
            HarvestCircleShellButton("Resume actual departure", presenter.departure::resume, Modifier.testTag("departure-resume"))
        }
        when {
            state.submission != null -> DispatchOutcome(state, presenter)
            state.formOpen && state.draft != null -> DispatchEditor(state, presenter, submit)
            state.selected != null -> DispatchRecord(state, presenter)
            else -> {
                state.draft?.let {
                    HarvestCircleShellButton(
                        "Resume identification draft",
                        presenter::resume,
                        Modifier.testTag("dispatch-resume"),
                    )
                }
                HarvestCircleShellButton("Prepare identification", {
                    presenter.begin(HubDispatchKind.Prepare)
                }, Modifier.testTag("dispatch-prepare"), primary = true, enabled = !state.loading)
                HarvestCircleShellButton("Reference data", references, enabled = !state.loading)
                HarvestCircleShellTextField(
                    state.queryText,
                    presenter::search,
                    "Search preparations",
                    "Recipient or observation",
                    inputModifier = Modifier.testTag("dispatch-search"),
                )
                HarvestCircleShellButton(
                    "Search / refresh",
                    presenter::refresh,
                    Modifier.testTag("dispatch-refresh"),
                    enabled = !state.loading,
                )
                HubDispatchFilter.entries.forEach { filter ->
                    HarvestCircleShellButton(
                        filter.name,
                        { presenter.filter(filter) },
                        Modifier.testTag("dispatch-filter-$filter"),
                        primary =
                            state.filter == filter,
                        enabled = !state.loading,
                    )
                }
                state.page?.let { page ->
                    HarvestCircleShellText("${page.total} preparations")
                    if (page.rows.isEmpty()) {
                        HarvestCircleShellText(
                            "No matching preparations. Stock remains available until an actual departure is recorded.",
                        )
                    }
                    page.rows.forEach { row ->
                        HarvestCircleShellPanel {
                            HarvestCircleShellText("${row.recipient.value.label} · ${row.state}")
                            HarvestCircleShellText(
                                "${row.wholePacks} original packs · ${row.lineCount} source lines · ${row.versionCount} issued versions",
                            )
                            HarvestCircleShellButton("Open ${row.id}", {
                                presenter.inspect(row.id)
                            }, Modifier.testTag("dispatch-record-${row.id}"), enabled = !state.loading)
                        }
                    }
                    if (page.more) {
                        HarvestCircleShellButton(
                            "Next preparations",
                            presenter::nextPage,
                            Modifier.testTag("dispatch-next"),
                            enabled = !state.loading,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun DispatchRecord(
    state: DispatchViewState,
    presenter: DispatchPresenter,
) {
    val view = requireNotNull(state.selected)
    val record = view.record
    HarvestCircleShellButton("Back to preparations", presenter::closeRecord, Modifier.testTag("dispatch-back"), enabled = !state.loading)
    HarvestCircleShellPanel(Modifier.testTag("dispatch-record")) {
        HarvestCircleShellText("Preparation ${record.id}", role = HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText("${view.state} · Record revision ${record.revision}")
        DispatchStamp(record.created, "Prepared")
        DispatchStamp(record.updated, "Last recorded change")
        record.draft?.let { DispatchMaterial(it) }
        when (view.state) {
            HubDispatchState.Draft -> {
                HarvestCircleShellButton("Revise preparation", {
                    presenter.begin(HubDispatchKind.Revise)
                }, Modifier.testTag("dispatch-revise"), enabled = !state.loading)
                HarvestCircleShellButton("Issue identification", {
                    presenter.begin(HubDispatchKind.Issue)
                }, Modifier.testTag("dispatch-issue"), primary = true, enabled = !state.loading)
            }
            HubDispatchState.Issued -> {
                if (presenter.departure.state.value.available) {
                    HarvestCircleShellButton(
                        "Record actual departure · Current version ${record.versions.lastOrNull()?.number}",
                        presenter::beginDeparture,
                        Modifier.testTag("dispatch-depart"),
                        primary = true,
                        enabled = !state.loading,
                    )
                }
                HarvestCircleShellButton("Replace identification", {
                    presenter.begin(HubDispatchKind.Replace)
                }, Modifier.testTag("dispatch-replace"), enabled = !state.loading)
                HarvestCircleShellButton("Mark identification unused", {
                    presenter.begin(HubDispatchKind.MarkUnused)
                }, Modifier.testTag("dispatch-unused"), enabled = !state.loading)
            }
            HubDispatchState.Departed -> {
                HarvestCircleShellText(
                    "Actual departure is recorded. Original issued material remains in history.",
                )
                view.departure?.let {
                    HarvestCircleShellText("Departure recorded: ${stockUtc(it.recordedAt.value)} · ${it.actor.value}")
                    HarvestCircleShellText("Original departure operation: ${it.operationId.value}")
                }
            }
            HubDispatchState.Unused -> HarvestCircleShellText("This identification is unused. Original issued material remains in history.")
        }
        HarvestCircleShellText(
            "Identification paperwork and the separately confirmed actual departure remain distinct facts.",
        )
    }
    record.versions.forEach { version ->
        HarvestCircleShellButton(
            "Version ${version.number} · ${version.status}",
            { presenter.version(version.id) },
            Modifier.testTag("dispatch-version-${version.id}"),
            primary =
                state.selectedVersionId == version.id,
            enabled = !state.loading,
        )
    }
    val version = record.versions.find { it.id == state.selectedVersionId } ?: record.versions.lastOrNull()
    version?.let {
        HarvestCircleShellPanel(Modifier.testTag("dispatch-issued-preview")) {
            HarvestCircleShellText(
                "Issued identification · Version ${it.number} · ${it.status}",
                role = HarvestCircleShellTextRole.SectionTitle,
            )
            HarvestCircleShellText("Identification: ${it.id}")
            it.previousId?.let { previous -> HarvestCircleShellText("Replaces: $previous") }
            DispatchStamp(it.issued, "Issued")
            it.closed?.let { closure -> DispatchStamp(closure, "Closed") }
            DispatchMaterial(it.material)
            HarvestCircleShellText("These are the captured as-issued facts. Current labels or stock can differ.")
        }
    }
}

@Composable
private fun DispatchOutcome(
    state: DispatchViewState,
    presenter: DispatchPresenter,
) {
    val result = requireNotNull(state.submission)
    HarvestCircleShellPanel {
        HarvestCircleShellText("Original operation: ${result.attempt.operation.value}")
        when (result) {
            is DispatchSubmission.Pending -> HarvestCircleShellText("Recording identification…", Modifier.testTag("dispatch-pending"))
            is DispatchSubmission.Recorded -> {
                HarvestCircleShellText(
                    "Identification recorded",
                    Modifier.testTag("dispatch-recorded"),
                    HarvestCircleShellTextRole.SectionTitle,
                )
                HarvestCircleShellText("${dispatchActionLabel(result.receipt.kind)} · ${result.receipt.record.state}")
                HarvestCircleShellText("Stock was not consumed and work was not completed by this action.")
                DispatchStamp(result.receipt.record.updated, "Recorded")
                HarvestCircleShellButton(
                    "View recorded identification",
                    presenter::finishRecorded,
                    Modifier.testTag("dispatch-done"),
                    primary = true,
                )
            }
            is DispatchSubmission.Rejected -> {
                HarvestCircleShellText(result.problem.safeMessage, Modifier.testTag("dispatch-rejected"))
                HarvestCircleShellButton(
                    "Review retained input",
                    presenter::reviewRejected,
                    Modifier.testTag("dispatch-review-rejected"),
                    enabled = !state.loading,
                )
                HarvestCircleShellButton(
                    "Check original result",
                    presenter::checkOriginal,
                    Modifier.testTag("dispatch-check-original"),
                    enabled = !state.loading,
                )
            }
            is DispatchSubmission.Uncertain -> {
                HarvestCircleShellText(result.message, Modifier.testTag("dispatch-uncertain"))
                HarvestCircleShellText("Original-result lookup does not submit or issue another identification.")
                HarvestCircleShellButton(
                    "Check original result",
                    presenter::checkOriginal,
                    Modifier.testTag("dispatch-check-original"),
                    enabled = !state.loading,
                )
            }
        }
    }
}
