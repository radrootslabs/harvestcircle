package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HandlingAction
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HandlingTab
import org.harvestcircle.application.HandlingViewState
import org.harvestcircle.application.HubDiscrepancyRow
import org.harvestcircle.application.HubGroupView
import org.harvestcircle.application.HubWorkOrigin
import org.harvestcircle.application.HubWorkProgress
import org.harvestcircle.application.HubWorkRow
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
fun HandlingWorkspace(
    state: HandlingViewState,
    presenter: HandlingPresenter,
    submit: () -> Unit,
    finish: () -> Unit,
    receive: () -> Unit,
    openStock: (String) -> Unit,
    openReferences: () -> Unit,
    receiveEnabled: Boolean = true,
    receiveLabel: String = "Receive original packs",
    openTrace: ((String) -> Unit)? = null,
    browseTrace: (() -> Unit)? = null,
) {
    val scroll = rememberScrollState()
    LaunchedEffect(handlingSurface(state)) { scroll.scrollTo(0) }
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(scroll)
            .padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("handling-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleShellText("Sample data only · Whole original packs · One desktop")
        browseTrace?.let {
            HarvestCircleShellButton(
                "Browse trace and history",
                it,
                Modifier.testTag("trace-browse"),
                enabled = !state.loading,
            )
        }
        val traceId = state.group?.trace?.id ?: state.selectedWork?.work?.traceId ?: state.selectedDiscrepancy?.value?.traceId
        if (traceId != null &&
            openTrace != null
        ) {
            HarvestCircleShellButton(
                "Trace and history",
                { openTrace(traceId) },
                Modifier.testTag("handling-trace"),
                enabled = !state.loading,
            )
        }
        state.message?.let { HarvestCircleShellText(it, Modifier.testTag("handling-message")) }
        if (state.loading) HarvestCircleShellText("Reading local handling records…", Modifier.testTag("handling-loading"))
        when {
            state.submission != null -> HandlingOutcome(state, presenter, finish)
            state.formOpen && state.draft != null -> HandlingEditor(state, presenter, submit)
            else -> {
                state.draft?.let {
                    HarvestCircleShellButton("Resume handling draft", presenter::resumeDraft, Modifier.testTag("handling-resume"))
                }
                when {
                    state.group != null -> HandlingGroupRecord(state.group, presenter, state)
                    state.selectedWork != null -> HandlingWorkRecord(state.selectedWork, presenter, openStock)
                    state.selectedDiscrepancy != null -> HandlingDiscrepancyRecord(state.selectedDiscrepancy, presenter)
                    else -> HandlingQueues(state, presenter, receive, openReferences, receiveEnabled, receiveLabel)
                }
            }
        }
    }
}

@Composable
private fun HandlingQueues(
    state: HandlingViewState,
    presenter: HandlingPresenter,
    receive: () -> Unit,
    openReferences: () -> Unit,
    receiveEnabled: Boolean,
    receiveLabel: String,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton(
            receiveLabel,
            receive,
            Modifier.testTag("stock-receive"),
            primary = true,
            enabled =
                !state.loading && receiveEnabled,
        )
        HarvestCircleShellButton("Reference data", openReferences, enabled = !state.loading)
    }
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton(
            "Work",
            { presenter.chooseTab(HandlingTab.Work) },
            Modifier.testTag("handling-work-tab"),
            primary =
                state.tab == HandlingTab.Work,
            enabled = !state.loading,
        )
        HarvestCircleShellButton(
            "Discrepancies",
            { presenter.chooseTab(HandlingTab.Discrepancies) },
            Modifier.testTag("handling-discrepancies-tab"),
            primary =
                state.tab == HandlingTab.Discrepancies,
            enabled = !state.loading,
        )
    }
    HarvestCircleShellTextField(
        state.queryText,
        presenter::search,
        if (state.tab == HandlingTab.Work) "Find work by product" else "Find discrepancy observations",
        "Search local records",
        inputModifier = Modifier.testTag("handling-search"),
    )
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton("Search / refresh", presenter::refresh, Modifier.testTag("handling-refresh"), enabled = !state.loading)
        HarvestCircleShellButton(
            if (state.openOnly) "Include closed work" else "Show remaining only",
            { presenter.showClosed(state.openOnly) },
            Modifier.testTag("handling-include-closed"),
            enabled = !state.loading,
        )
    }
    when (state.tab) {
        HandlingTab.Work ->
            state.work?.let { page ->
                HarvestCircleShellText("${page.total} matching requirements", role = HarvestCircleShellTextRole.SectionTitle)
                if (page.rows.isEmpty()) HarvestCircleShellText("No handling work matches this view.")
                page.rows.forEach { row ->
                    HarvestCircleShellPanel(Modifier.testTag("work-row-${row.work.id}")) {
                        HarvestCircleShellText(row.product.value.label, role = HarvestCircleShellTextRole.BodyStrong)
                        HarvestCircleShellText("${row.variant.value.label} · ${workOrigin(row.origin)}")
                        HandlingQuantitySummary(row.work.quantity)
                        HarvestCircleShellText("${row.pendingGroups} groups with remaining work")
                        if (row.sourceNeedsReview) HarvestCircleShellText("Source review needed")
                        HarvestCircleShellButton(
                            "View requirement",
                            { presenter.selectWork(row) },
                            Modifier.testTag("work-inspect-${row.work.id}"),
                        )
                    }
                }
                HandlingPageButtons(presenter, state.loading, page.offset, page.more)
            }
        HandlingTab.Discrepancies ->
            state.discrepancies?.let { page ->
                HarvestCircleShellText("${page.total} matching discrepancies", role = HarvestCircleShellTextRole.SectionTitle)
                if (page.rows.isEmpty()) HarvestCircleShellText("No discrepancy follow-up matches this view.")
                page.rows.forEach { row ->
                    HarvestCircleShellPanel(Modifier.testTag("discrepancy-row-${row.value.id}")) {
                        HarvestCircleShellText(row.product.value.label, role = HarvestCircleShellTextRole.BodyStrong)
                        HarvestCircleShellText(row.value.observation)
                        HarvestCircleShellText(if (row.value.resolution == null) "Open follow-up" else "Resolved follow-up")
                        HarvestCircleShellButton(
                            "View discrepancy",
                            { presenter.selectDiscrepancy(row) },
                            Modifier.testTag("discrepancy-inspect-${row.value.id}"),
                        )
                    }
                }
                HandlingPageButtons(presenter, state.loading, page.offset, page.more)
            }
    }
}

@Composable
private fun HandlingPageButtons(
    presenter: HandlingPresenter,
    loading: Boolean,
    offset: UInt,
    more: Boolean,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton("First page", presenter::refresh, enabled = !loading && offset != 0U)
        HarvestCircleShellButton("Next page", presenter::nextPage, Modifier.testTag("handling-next"), enabled = !loading && more)
    }
}

@Composable
internal fun HandlingQuantitySummary(work: org.harvestcircle.application.HubQuantityWork) {
    HarvestCircleShellText("Target: ${work.target?.value?.label ?: "Not planned"}")
    HarvestCircleShellText("Required ${work.required} · Completed ${work.completed}")
    HarvestCircleShellText(
        "No longer required ${work.noLongerRequired} · Remaining ${work.remaining}",
        Modifier.testTag("handling-work-quantity"),
    )
}

@Composable
private fun HandlingGroupRecord(
    view: HubGroupView,
    presenter: HandlingPresenter,
    state: HandlingViewState,
) {
    HarvestCircleShellButton(
        "Back to stock / work",
        presenter::closeRecord,
        Modifier.testTag("handling-close-record"),
        enabled = !state.loading,
    )
    HarvestCircleShellText(view.product.value.label, role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellText("${view.group.wholePacks} original packs · ${view.variant.value.label}")
    HarvestCircleShellText("At: ${view.location.value.label}", Modifier.testTag("handling-group-location"))
    view.group.identification?.let {
        HarvestCircleShellText("Physical identifier: ${it.label}")
        HarvestCircleShellText("Identification observation: ${it.observation}")
    }
    HarvestCircleShellText("Group: ${view.group.id}")
    view.group.parentId?.let { HarvestCircleShellText("Separated from: $it") }
    val actions =
        state.treatment
            ?.groups
            ?.find { it.groupId == view.group.id }
            ?.actions
    HarvestCircleShellButton("Record actual movement", {
        presenter.begin(HandlingAction.Move)
    }, Modifier.testTag("handling-begin-move"), primary = true, enabled = !state.loading && actions?.internalRelocation == true)
    HarvestCircleShellButton("Separate identified portion", {
        presenter.begin(HandlingAction.Split)
    }, Modifier.testTag("handling-begin-split"), enabled = !state.loading && actions?.observedSeparation == true)
    HarvestCircleShellButton("New handling requirement", {
        presenter.begin(HandlingAction.NewWork)
    }, Modifier.testTag("handling-begin-new-work"), enabled = !state.loading)
    HarvestCircleShellButton("Report discrepancy", {
        presenter.begin(HandlingAction.OpenDiscrepancy)
    }, Modifier.testTag("handling-begin-discrepancy"), enabled = !state.loading)
    view.group.work.forEach { allocation ->
        HarvestCircleShellPanel {
            HarvestCircleShellText("This group's work: ${workProgress(allocation.progress)}", role = HarvestCircleShellTextRole.BodyStrong)
            HarvestCircleShellText("Requirement totals across its groups")
            HandlingQuantitySummary(allocation.requirement.quantity)
            HarvestCircleShellText("Requirement: ${allocation.requirement.id}")
        }
    }
    state.treatment?.let { TreatmentRecord(it, state, presenter) }
    StockPopulationSummary(view.trace)
    state.lineage?.let { PortionLineage(it, state.loading, presenter, view.group.id) }
}

@Composable
private fun HandlingWorkRecord(
    row: HubWorkRow,
    presenter: HandlingPresenter,
    openStock: (String) -> Unit,
) {
    HarvestCircleShellButton("Back to work", presenter::closeRecord, Modifier.testTag("handling-close-record"))
    HarvestCircleShellText(row.product.value.label, role = HarvestCircleShellTextRole.SectionTitle)
    HandlingQuantitySummary(row.work.quantity)
    HarvestCircleShellText("Original basis: ${row.observation}")
    HarvestCircleShellText("Created: ${stockUtc(row.createdAt.value)} · ${row.createdBy.value}")
    HarvestCircleShellText("Replanning changes all remaining work on this requirement. It does not move stock or reset completed work.")
    HarvestCircleShellButton(
        "Replan remaining work",
        { presenter.begin(HandlingAction.Replan) },
        Modifier.testTag("handling-begin-replan"),
        enabled =
            row.work.quantity.remaining != 0U,
    )
    HarvestCircleShellButton("View this receipt's stock", { openStock(row.work.traceId) }, Modifier.testTag("handling-open-stock"))
}

@Composable
private fun HandlingDiscrepancyRecord(
    row: HubDiscrepancyRow,
    presenter: HandlingPresenter,
) {
    HarvestCircleShellButton("Back to discrepancies", presenter::closeRecord, Modifier.testTag("handling-close-record"))
    HarvestCircleShellText(row.product.value.label, role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellText("Original observation: ${row.value.observation}")
    HarvestCircleShellText("Reported: ${stockUtc(row.createdAt.value)} · ${row.createdBy.value}")
    HarvestCircleShellText("Closing this follow-up does not change stock, complete work or release independent holds.")
    row.value.resolution?.let { HarvestCircleShellText("Resolution: $it") }
    row.resolvedAt?.let { HarvestCircleShellText("Resolved: ${stockUtc(it.value)} · ${row.resolvedBy?.value}") }
    HarvestCircleShellButton(
        "Resolve follow-up",
        { presenter.begin(HandlingAction.ResolveDiscrepancy) },
        Modifier.testTag("handling-begin-resolve"),
        enabled = row.value.resolution == null,
    )
}

private fun workOrigin(origin: HubWorkOrigin) =
    when (origin) {
        HubWorkOrigin.Intake -> "Receipt put-away"
        HubWorkOrigin.Replan -> "Replanned handling"
        HubWorkOrigin.NewHandling -> "New handling"
    }

private fun workProgress(progress: HubWorkProgress) =
    when (progress) {
        HubWorkProgress.Pending -> "Remaining"
        HubWorkProgress.Completed -> "Completed"
        HubWorkProgress.NoLongerRequired -> "No longer required"
    }

private fun handlingSurface(state: HandlingViewState): String =
    when {
        state.submission is org.harvestcircle.application.HandlingSubmission.Pending -> "pending"
        state.submission is org.harvestcircle.application.HandlingSubmission.Recorded -> "recorded"
        state.submission is org.harvestcircle.application.HandlingSubmission.Rejected -> "rejected"
        state.submission is org.harvestcircle.application.HandlingSubmission.Uncertain -> "uncertain"
        state.formOpen && state.draft != null -> handlingTitle(state.draft)
        state.group != null -> "group-${state.group.group.id}-${state.lineage?.offset}-${state.treatmentHistory?.offset}"
        state.selectedWork != null -> "work-${state.selectedWork.work.id}"
        state.selectedDiscrepancy != null -> "discrepancy-${state.selectedDiscrepancy.value.id}"
        else -> "queue-${state.tab}-${state.work?.offset}-${state.discrepancies?.offset}"
    }
