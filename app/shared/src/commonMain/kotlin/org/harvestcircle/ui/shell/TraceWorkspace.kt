package org.harvestcircle.ui.shell

import androidx.compose.foundation.focusable
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
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HubLocalOutputRequest
import org.harvestcircle.application.HubTraceHistoryKind
import org.harvestcircle.application.HubTraceScope
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.application.TraceSection
import org.harvestcircle.application.TraceViewState
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
internal fun TraceWorkspace(
    state: TraceViewState,
    presenter: TracePresenter,
    outputAvailable: Boolean = false,
) {
    val scroll = rememberScrollState()
    val focus = remember { FocusRequester() }
    LaunchedEffect(state.targetId, state.loading) {
        if (!state.loading) focus.requestFocus()
    }
    LaunchedEffect(state.targetId, state.section, state.page?.offset, state.history?.offset) { scroll.scrollTo(0) }
    Column(
        Modifier
            .fillMaxSize()
            .focusRequester(focus)
            .focusable()
            .verticalScroll(scroll)
            .padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("trace-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleShellText("Sample data only · Recorded local trace", role = HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellButton("Back to workspace", presenter::park, Modifier.testTag("trace-close"))
        state.message?.let { HarvestCircleShellText(it, Modifier.testTag("trace-message")) }
        if (state.loading) HarvestCircleShellText("Reading the selected trace context…", Modifier.testTag("trace-loading"))
        if (state.needsRefresh) HarvestCircleShellText("This captured context needs refresh. The selected population or scope is retained.")
        HarvestCircleShellButton(
            "Refresh selected context",
            presenter::refresh,
            Modifier.testTag("trace-refresh"),
            enabled = !state.loading,
        )
        if (state.targetId != null) {
            HarvestCircleShellButton(
                "Browse recorded populations",
                presenter::closeRecord,
                Modifier.testTag("trace-back-list"),
                enabled = !state.loading,
            )
            state.snapshot?.let { snapshot ->
                HarvestCircleShellText(
                    "${snapshot.trace.product.value.label} · ${snapshot.trace.variant.value.label}",
                    role = HarvestCircleShellTextRole.SectionTitle,
                )
                HarvestCircleShellText("Captured stock ${snapshot.stockRevision} · Catalog ${snapshot.catalogRevision}")
                if (outputAvailable) {
                    HarvestCircleShellButton(
                        "Save or print trace snapshot",
                        {
                            presenter.output.prepare(
                                HubLocalOutputRequest.Trace(snapshot.trace.id, snapshot.stockRevision, snapshot.catalogRevision),
                            )
                        },
                        Modifier.testTag("trace-output"),
                        enabled = !state.loading && !state.needsRefresh,
                    )
                }
                HarvestCircleShellText(
                    "Original ${snapshot.trace.originalWholePacks} · On hand ${snapshot.onHandWholePacks} · Actual departed ${snapshot.departedWholePacks}",
                    Modifier.testTag("trace-balance"),
                )
                HarvestCircleShellText(
                    "Counts belong to this population and pack unit. Unknown origin and unrecorded or off-device activity cannot be ruled out.",
                )
                Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                    TraceSection.entries.forEach { section ->
                        HarvestCircleShellButton(
                            section.name,
                            { presenter.chooseSection(section) },
                            Modifier.testTag("trace-section-$section"),
                            primary =
                                state.section == section,
                        )
                    }
                }
                when (state.section) {
                    TraceSection.Overview -> TraceOverview(state, presenter)
                    TraceSection.History -> TraceHistory(state, presenter)
                }
            }
        } else {
            TraceBrowser(state, presenter)
        }
    }
}

@Composable
private fun TraceBrowser(
    state: TraceViewState,
    presenter: TracePresenter,
) {
    HarvestCircleShellButton("All recorded populations", { presenter.browse() }, Modifier.testTag("trace-all"), enabled = !state.loading)
    HarvestCircleShellButton("Unresolved source populations", {
        presenter.browse(HubTraceScope.Unresolved)
    }, Modifier.testTag("trace-unresolved"), enabled = !state.loading)
    if (state.selection.scope == HubTraceScope.SourceImpact) {
        HarvestCircleShellText(
            "Source impact candidates: exact selected source plus every unresolved population. Inclusion does not establish a source relationship.",
        )
        HarvestCircleShellText("Selected source: ${state.selection.sourceId}")
    } else {
        HarvestCircleShellTextField(
            state.queryText,
            presenter::search,
            "Search recorded original fields",
            "Original product, pack, issuer or source code",
            inputModifier = Modifier.testTag("trace-search"),
        )
        HarvestCircleShellButton("Search", presenter::refresh, Modifier.testTag("trace-search-submit"), enabled = !state.loading)
    }
    HarvestCircleShellText(
        "Local recorded scope only. Includes fully departed populations; matching code text does not merge issuers or source identities.",
    )
    state.page?.let { page ->
        HarvestCircleShellText(
            "${page.total} matching populations · Captured stock ${page.stockRevision} · Catalog ${page.catalogRevision}",
        )
        if (page.selection.text.isNotEmpty()) HarvestCircleShellText("Results for original fields: ${page.selection.text}")
        if (page.rows.isEmpty()) {
            HarvestCircleShellText(
                "No stored population matches this selection. This does not establish absence of unrecorded activity.",
            )
        }
        page.rows.forEach { row ->
            HarvestCircleShellPanel {
                HarvestCircleShellText(
                    "${row.trace.product.value.label} · ${row.trace.variant.value.label}",
                    role = HarvestCircleShellTextRole.BodyStrong,
                )
                HarvestCircleShellText(
                    "Original ${row.trace.originalWholePacks} · On hand ${row.onHandWholePacks} · Actual departed ${row.departedWholePacks}",
                )
                HarvestCircleShellText(
                    "Source: ${row.trace.source.certainty} · ${row.trace.source.code ?: "Unknown"} · Issuer ${row.trace.source.issuer
                        ?.value
                        ?.label ?: "Unknown"}",
                )
                if (row.trace.sourceNeedsReview) HarvestCircleShellText("Source review needed")
                HarvestCircleShellButton("Inspect population", {
                    presenter.openReceipt(row.trace.id)
                }, Modifier.testTag("trace-open-${row.trace.id}"), enabled = !state.loading)
            }
        }
        HarvestCircleShellText(
            "This page contains ${page.rows.size} of ${page.total} matches. Later pages remain part of the selected scope.",
        )
        if (page.more) {
            HarvestCircleShellButton(
                "Next populations",
                presenter::nextPage,
                Modifier.testTag("trace-next"),
                enabled = !state.loading,
            )
        }
    }
}

@Composable
private fun TraceOverview(
    state: TraceViewState,
    presenter: TracePresenter,
) {
    val snapshot = requireNotNull(state.snapshot)
    ReceiptTraceSummary(snapshot.trace)
    snapshot.trace.source.id?.let { source ->
        HarvestCircleShellButton("Find this source and all unresolved candidates", {
            presenter.browse(HubTraceScope.SourceImpact, source)
        }, Modifier.testTag("trace-source-impact"), enabled = !state.loading)
    }
    HarvestCircleShellText("Current on-hand groups", role = HarvestCircleShellTextRole.SectionTitle)
    if (snapshot.onHand.isEmpty()) HarvestCircleShellText("No packs remain on hand in this population.")
    snapshot.onHand.drop(state.positionsOffset).take(64).forEach { position ->
        HarvestCircleShellPanel {
            HarvestCircleShellText("${position.wholePacks} original packs · ${position.location.value.label}")
            HarvestCircleShellText(position.identification?.label ?: "Original received group")
            HarvestCircleShellText("Sample acceptance: ${position.acceptance} · Active holds ${position.activeHoldIds.size}")
            position.activeHoldIds.forEach { HarvestCircleShellText("Hold reference: $it") }
            HarvestCircleShellText("Group ${position.groupId} · Revision ${position.groupRevision}")
        }
    }
    if (snapshot.onHand.size > 64) {
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellButton("Previous groups", { presenter.positionsPage(-1) }, enabled = state.positionsOffset > 0)
            HarvestCircleShellButton(
                "Next groups",
                { presenter.positionsPage(1) },
                enabled =
                    state.positionsOffset + 64 < snapshot.onHand.size,
            )
        }
    }
    HarvestCircleShellText("Retained actual departure lines", role = HarvestCircleShellTextRole.SectionTitle)
    if (snapshot.exits.isEmpty()) HarvestCircleShellText("No recorded actual departure in this population.")
    snapshot.exits.drop(state.exitsOffset).take(64).forEach { exit ->
        HarvestCircleShellPanel {
            HarvestCircleShellText(
                "${exit.wholePacks} original packs · Recipient ${exit.recipient.value.label}",
                role = HarvestCircleShellTextRole.BodyStrong,
            )
            HarvestCircleShellText(exit.observation)
            HarvestCircleShellText("Recorded ${stockUtc(exit.recordedAt.value)} · ${exit.actor.value}")
            HarvestCircleShellText("Actual occurrence: ${exit.occurrence.kind}")
            exit.occurrence.startUnixSeconds?.let { HarvestCircleShellText("Start: ${stockUtc(it)}") }
            exit.occurrence.endUnixSeconds?.let { HarvestCircleShellText("End: ${stockUtc(it)}") }
            HarvestCircleShellText("Group ${exit.groupId} · Preparation ${exit.preparationId}")
            HarvestCircleShellText("Identification ${exit.identificationId}")
            SelectionContainer { HarvestCircleShellText("Original operation ${exit.operationId.value}") }
        }
    }
    if (snapshot.exits.size > 64) {
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellButton("Previous departures", { presenter.exitsPage(-1) }, enabled = state.exitsOffset > 0)
            HarvestCircleShellButton("Next departures", { presenter.exitsPage(1) }, enabled = state.exitsOffset + 64 < snapshot.exits.size)
        }
    }
}

@Composable
private fun TraceHistory(
    state: TraceViewState,
    presenter: TracePresenter,
) {
    HarvestCircleShellText("Recorded action history", role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellText(
        "Recording order does not establish actual occurrence order. This page includes linked before/after document material; it is not an external audit or delivery acknowledgement.",
    )
    state.history?.let { page ->
        HarvestCircleShellText("${page.total} linked actions · This page ${page.rows.size} · Starting offset ${page.offset}")
        if (page.rows.isEmpty()) HarvestCircleShellText("No linked action on this page.")
        page.rows.forEach { entry ->
            HarvestCircleShellPanel {
                HarvestCircleShellText(traceActionLabel(entry.kind), role = HarvestCircleShellTextRole.BodyStrong)
                HarvestCircleShellText(entry.observation)
                HarvestCircleShellText("Recorded ${stockUtc(entry.recordedAt.value)} · ${entry.actor.value}")
                val occurrence = entry.occurrence
                if (occurrence == null) {
                    HarvestCircleShellText("This action records no physical occurrence.")
                } else {
                    HarvestCircleShellText("Actual occurrence: ${occurrence.kind}")
                    occurrence.startUnixSeconds?.let { HarvestCircleShellText("Start: ${stockUtc(it)}") }
                    occurrence.endUnixSeconds?.let { HarvestCircleShellText("End: ${stockUtc(it)}") }
                }
                SelectionContainer { HarvestCircleShellText("Original operation ${entry.operationId.value}") }
                HarvestCircleShellText("Recorded stock revision ${entry.stockRevision} · Result reference ${entry.resultId}")
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellButton(
                "First history page",
                presenter::historyFirst,
                enabled =
                    !state.loading && !state.needsRefresh && page.offset != 0U,
            )
            if (page.more) {
                HarvestCircleShellButton(
                    "Next history page",
                    presenter::historyNext,
                    Modifier.testTag("trace-history-next"),
                    enabled =
                        !state.loading && !state.needsRefresh,
                )
            }
        }
    }
}

private fun traceActionLabel(kind: HubTraceHistoryKind): String =
    when (kind) {
        HubTraceHistoryKind.Receipt -> "Original receipt"
        HubTraceHistoryKind.Move -> "Actual movement"
        HubTraceHistoryKind.Split -> "Observed physical separation"
        HubTraceHistoryKind.Acceptance -> "Sample acceptance decision"
        HubTraceHistoryKind.PlaceHold -> "Hold placed"
        HubTraceHistoryKind.ReleaseHold -> "Selected hold released"
        HubTraceHistoryKind.Replan -> "Handling replanned"
        HubTraceHistoryKind.NewWork -> "New handling requirement"
        HubTraceHistoryKind.OpenDiscrepancy -> "Discrepancy observed"
        HubTraceHistoryKind.ResolveDiscrepancy -> "Discrepancy follow-up resolved"
        HubTraceHistoryKind.PrepareIdentification -> "Identification prepared"
        HubTraceHistoryKind.ReviseIdentification -> "Preparation revised"
        HubTraceHistoryKind.IssueIdentification -> "Identification issued"
        HubTraceHistoryKind.ReplaceIdentification -> "Identification replaced"
        HubTraceHistoryKind.MarkIdentificationUnused -> "Identification marked unused"
        HubTraceHistoryKind.Departure -> "Actual departure recorded"
    }
