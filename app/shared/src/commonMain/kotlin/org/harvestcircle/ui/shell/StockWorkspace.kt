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
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubStockRow
import org.harvestcircle.application.ReceiptSubmission
import org.harvestcircle.application.StockPresenter
import org.harvestcircle.application.StockViewState
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.harvestcircle.navigation.AppRoute
import kotlin.time.Instant

@Composable
fun StockWorkspace(
    route: AppRoute,
    state: StockViewState,
    presenter: StockPresenter,
    submit: () -> Unit,
    openReferences: () -> Unit,
    openHandling: ((String) -> Unit)? = null,
    manageRoute: Boolean = true,
    handlingEnabled: Boolean = true,
    openTrace: ((String) -> Unit)? = null,
    browseTrace: (() -> Unit)? = null,
) {
    LaunchedEffect(route) {
        if (manageRoute) {
            presenter.hideReceipt()
            presenter.closeInspection()
        }
    }
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("stock-workspace"),
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
        val traceId =
            state.inspecting?.trace?.id
                ?: (state.submission as? org.harvestcircle.application.ReceiptSubmission.Recorded)?.receipt?.trace?.id
        if (traceId != null &&
            openTrace != null
        ) {
            HarvestCircleShellButton(
                "Trace and history",
                { openTrace(traceId) },
                Modifier.testTag("stock-trace"),
                enabled = !state.loading,
            )
        }
        state.page?.let {
            HarvestCircleShellText(
                "${it.handlingBasis.label} v${it.handlingBasis.version} · ${it.handlingBasis.siteTimezone}",
            )
        }
        HarvestCircleShellButton("Reference data", openReferences, Modifier.testTag("stock-open-references"))
        state.message?.let { HarvestCircleShellText(it, Modifier.testTag("stock-message")) }
        if (state.loading) HarvestCircleShellText("Reading local stock…", Modifier.testTag("stock-loading"))
        when {
            state.submission != null -> ReceiptOutcomePanel(state, presenter)
            state.receiptOpen && state.draft != null -> ReceiptEditor(state, presenter, submit)
            state.inspecting != null -> StockRecord(state.inspecting, presenter, openHandling, handlingEnabled)
            else -> StockRows(route, state, presenter)
        }
    }
}

@Composable
private fun StockRows(
    route: AppRoute,
    state: StockViewState,
    presenter: StockPresenter,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton(
            if (state.draft != null) "Resume receipt draft" else "Receive original packs",
            { presenter.beginReceipt() },
            Modifier.testTag("stock-receive"),
            primary = true,
            enabled = !state.loading && state.page != null,
        )
        HarvestCircleShellButton("Refresh", presenter::refresh, Modifier.testTag("stock-refresh"), enabled = !state.loading)
    }
    state.traceFilter?.let {
        HarvestCircleShellText("Stock from selected receipt: $it")
        HarvestCircleShellButton("Show all receipts", {
            presenter.showTrace(null)
        }, Modifier.testTag("stock-clear-trace"), enabled = !state.loading)
    }
    HarvestCircleShellTextField(
        state.queryText,
        presenter::search,
        "Find stock",
        "Product, original pack or source code",
        inputModifier = Modifier.testTag("stock-search"),
    )
    HarvestCircleShellButton("Search", presenter::refresh, Modifier.testTag("stock-search-submit"), enabled = !state.loading)
    ReferenceChoice(
        "Location",
        "stock-location-filter",
        state.references,
        HubReferenceKind.Location,
        state.locationFilter,
        presenter::filterLocation,
        emptyLabel = "All locations",
    )
    val page = state.page ?: return
    HarvestCircleShellText(
        if (route == AppRoute.HubWork) "Intake work in this stock view" else "Stock positions",
        role = HarvestCircleShellTextRole.SectionTitle,
    )
    HarvestCircleShellText("${page.total} matching positions · Local stock version ${page.stockRevision}")
    if (page.rows.isEmpty()) HarvestCircleShellText("No stock matches this view. Record an actual receipt or change the filters.")
    page.rows.forEach { row ->
        HarvestCircleShellPanel(Modifier.testTag("stock-row-${row.position.groupId}-${row.position.locationId}")) {
            HarvestCircleShellText(row.product.value.label, role = HarvestCircleShellTextRole.BodyStrong)
            HarvestCircleShellText("${row.position.wholePacks} original packs · ${row.trace.variant.value.label}")
            HarvestCircleShellText("At: ${row.location.value.label}")
            if (row.trace.sourceNeedsReview) HarvestCircleShellText("Source review needed")
            if (route == AppRoute.HubWork) WorkSummary(row)
            HarvestCircleShellButton("View record", { presenter.inspect(row) }, Modifier.testTag("stock-inspect-${row.position.groupId}"))
        }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton("First page", presenter::refresh, enabled = !state.loading && page.offset != 0U)
        HarvestCircleShellButton("Next page", presenter::nextPage, Modifier.testTag("stock-next"), enabled = !state.loading && page.more)
    }
}

@Composable
private fun WorkSummary(row: HubStockRow) {
    val work = row.putAway
    HarvestCircleShellText(
        "Put-away · ${work.target?.value?.label ?: "Destination not planned"}",
        role = HarvestCircleShellTextRole.BodyStrong,
    )
    HarvestCircleShellText("Required ${work.required} · Completed ${work.completed}")
    HarvestCircleShellText(
        "No longer required ${work.noLongerRequired} · Remaining ${work.remaining}",
        Modifier.testTag("stock-work-remaining"),
    )
    HarvestCircleShellText("Totals for the original intake requirement. The handling record shows current scoped work and actual movement.")
}

@Composable
private fun StockRecord(
    row: HubStockRow,
    presenter: StockPresenter,
    openHandling: ((String) -> Unit)?,
    handlingEnabled: Boolean,
) {
    HarvestCircleShellButton("Back to stock view", presenter::closeInspection, Modifier.testTag("stock-close-record"))
    HarvestCircleShellText(row.product.value.label, role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellText("On hand here: ${row.position.wholePacks} original packs · ${row.location.value.label}")
    WorkSummary(row)
    openHandling?.let { open ->
        HarvestCircleShellButton("Handling and location", {
            open(row.position.groupId)
        }, Modifier.testTag("stock-handling-${row.position.groupId}"), primary = true, enabled = handlingEnabled)
    }
    ReceiptTraceSummary(row.trace)
    HarvestCircleShellButton(
        if (row.trace.source.id == null) "Receive another intake" else "Receive more with this source",
        { presenter.beginReceipt(row) },
        Modifier.testTag("stock-copy-source"),
    )
}

@Composable
internal fun ReceiptTraceSummary(trace: org.harvestcircle.application.HubReceiptTrace) {
    HarvestCircleShellPanel(Modifier.testTag("receipt-trace")) {
        HarvestCircleShellText("Original receipt", role = HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText("${trace.originalWholePacks} original packs · ${trace.variant.value.label}")
        HarvestCircleShellText("Product when received: ${trace.product.value.label}")
        HarvestCircleShellText("Received at: ${trace.receiving.value.label}")
        HarvestCircleShellText("Planned destination: ${trace.plannedLocation?.value?.label ?: "Not planned"}")
        HarvestCircleShellText("Supplier: ${trace.supplier?.value?.label ?: "Unknown"}")
        HarvestCircleShellText("Producer: ${trace.producer?.value?.label ?: "Unknown"}")
        HarvestCircleShellText("Source: ${trace.source.certainty} · ${trace.source.code ?: "Unknown"}")
        HarvestCircleShellText(
            "Source issuer: ${trace.source.issuer
                ?.value
                ?.label ?: "Unknown"}",
        )
        trace.source.provenance?.let { HarvestCircleShellText("Source observation: $it") }
        HarvestCircleShellText("Manual confirmation: ${trace.observation}")
        HarvestCircleShellText("Recorded: ${stockUtc(trace.recordedAt.value)}")
        HarvestCircleShellText(
            "Occurrence: ${trace.occurrence.kind}${trace.occurrence.startUnixSeconds?.let { " · ${stockUtc(it)}" }.orEmpty()}" +
                trace.occurrence.endUnixSeconds
                    ?.let { " to ${stockUtc(it)}" }
                    .orEmpty(),
        )
        HarvestCircleShellText("Recorded by: ${trace.actor.value}")
        HarvestCircleShellText("Receipt: ${trace.id}")
    }
}

@Composable
private fun ReceiptOutcomePanel(
    state: StockViewState,
    presenter: StockPresenter,
) {
    val submitted = requireNotNull(state.submission)
    HarvestCircleShellPanel(Modifier.testTag("receipt-outcome")) {
        HarvestCircleShellText("Original operation: ${submitted.attempt.operation.value}")
        HarvestCircleShellText("Submitted count: ${submitted.attempt.input.wholePacks} original packs")
        when (submitted) {
            is ReceiptSubmission.Pending -> HarvestCircleShellText("Recording the actual receipt…")
            is ReceiptSubmission.Recorded -> {
                HarvestCircleShellText("Receipt recorded", Modifier.testTag("receipt-recorded"), HarvestCircleShellTextRole.SectionTitle)
                HarvestCircleShellText(
                    "${submitted.receipt.initialPosition.wholePacks} original packs at ${submitted.receipt.trace.receiving.value.label}",
                )
                HarvestCircleShellText("Put-away remaining: ${submitted.receipt.initialPutAway.remaining}")
                ReceiptTraceSummary(submitted.receipt.trace)
                HarvestCircleShellButton("View current stock", presenter::finishRecorded, Modifier.testTag("receipt-done"), primary = true)
            }
            is ReceiptSubmission.Rejected -> {
                HarvestCircleShellText(submitted.problem.safeMessage)
                HarvestCircleShellButton(
                    "Review retained input",
                    presenter::reviewRejected,
                    Modifier.testTag("receipt-review"),
                    enabled = !state.loading,
                )
                HarvestCircleShellButton("Check original result", presenter::checkOriginal, enabled = !state.loading)
            }
            is ReceiptSubmission.Uncertain -> {
                HarvestCircleShellText(submitted.message, Modifier.testTag("receipt-uncertain"))
                HarvestCircleShellText("Checking reads the original result. It does not record another physical receipt.")
                HarvestCircleShellButton(
                    "Check original result",
                    presenter::checkOriginal,
                    Modifier.testTag("receipt-check-original"),
                    enabled = !state.loading,
                )
            }
        }
    }
}

internal fun stockUtc(seconds: Long): String = Instant.fromEpochSeconds(seconds).toString()
