package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HubLocationPurpose
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceRecord
import org.harvestcircle.application.ReferencePresenter
import org.harvestcircle.application.ReferenceSubmission
import org.harvestcircle.application.ReferenceViewState
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMenuButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import kotlin.time.Instant

@Composable
fun ReferenceWorkspace(
    state: ReferenceViewState,
    presenter: ReferencePresenter,
    submit: () -> Unit,
    returnToWork: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("reference-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleShellButton("Back to Work", returnToWork, Modifier.testTag("references-back"))
        state.page?.handlingBasis?.let { basis ->
            HarvestCircleShellText("Sample data · ${basis.label} v${basis.version} · Times shown in ${basis.siteTimezone}")
        }
        state.message?.let { HarvestCircleShellText(it, Modifier.testTag("reference-message")) }
        if (!state.available) {
            HarvestCircleShellText("Activate the hub owner's identity and initialize the demo hub to manage reference data.")
            return@Column
        }
        if (state.loading) HarvestCircleShellText("Reading local records…", Modifier.testTag("reference-loading"))
        when {
            state.submission != null -> ReferenceSubmissionPanel(state, presenter)
            state.inspecting != null -> ReferenceHistoryPanel(state, presenter)
            state.draft != null -> ReferenceEditor(state, presenter, submit)
            else -> ReferenceList(state, presenter)
        }
    }
}

@Composable
private fun ReferenceList(
    state: ReferenceViewState,
    presenter: ReferencePresenter,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HubReferenceKind.entries.forEach { kind ->
            HarvestCircleShellButton(
                kind.label,
                { presenter.selectKind(kind) },
                Modifier.testTag("reference-kind-${kind.name}"),
                primary = state.kind == kind,
                enabled = !state.loading,
            )
        }
    }
    HarvestCircleShellTextField(
        state.queryText,
        presenter::search,
        "Find ${state.kind.label.lowercase()}",
        "Name or label",
        inputModifier = Modifier.testTag("reference-search"),
    )
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton("Search / refresh", presenter::reload, Modifier.testTag("reference-refresh"), enabled = !state.loading)
        HarvestCircleShellButton(
            "Add reference",
            presenter::create,
            Modifier.testTag("reference-add"),
            primary = true,
            enabled = !state.loading && state.page != null,
        )
    }
    val page = state.page ?: return
    HarvestCircleShellText("${page.total} reference records · ${state.kind.label}", role = HarvestCircleShellTextRole.SectionTitle)
    if (page.records.isEmpty()) HarvestCircleShellText("No matching references. Add the records needed for your fictional hub.")
    page.records.forEach { record ->
        HarvestCircleShellPanel(Modifier.testTag("reference-row-${record.id}")) {
            ReferenceSummary(record)
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                HarvestCircleShellButton("History", { presenter.inspect(record) }, enabled = !state.loading)
                if (record.value.kind != HubReferenceKind.Pack) {
                    HarvestCircleShellButton("Revise label", { presenter.edit(record) }, enabled = !state.loading)
                }
            }
        }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton("First page", presenter::firstPage, enabled = !state.loading && page.offset != 0U)
        HarvestCircleShellButton(
            "Next page",
            presenter::nextPage,
            Modifier.testTag("reference-next"),
            enabled = !state.loading && page.more,
        )
    }
}

@Composable
private fun ReferenceEditor(
    state: ReferenceViewState,
    presenter: ReferencePresenter,
    submit: () -> Unit,
) {
    val draft = requireNotNull(state.draft)
    HarvestCircleShellPanel(Modifier.testTag("reference-editor")) {
        HarvestCircleShellText(
            if (draft.recordId == null) "Add ${draft.kind.label.lowercase()}" else "Revise label",
            role = HarvestCircleShellTextRole.SectionTitle,
        )
        HarvestCircleShellTextField(
            draft.label,
            { text -> presenter.updateDraft { it.copy(label = text) } },
            "Label",
            "Public name within this local demo",
            inputModifier = Modifier.testTag("reference-label"),
        )
        if (draft.kind == HubReferenceKind.Pack) {
            HarvestCircleShellText(
                "An original pack is a distinct variant. A changed pack size needs another variant; nominal mass is not measured stock.",
            )
            HarvestCircleShellTextField(
                state.productSearch,
                presenter::changeProductSearch,
                "Find a product",
                "Product name",
                inputModifier = Modifier.testTag("reference-product-search"),
            )
            HarvestCircleShellButton("Find products", { presenter.searchProducts(state.productSearch) })
            HarvestCircleShellText(
                "Selected product: ${state.products
                    ?.records
                    ?.find { it.id == draft.productId }
                    ?.value
                    ?.label ?: draft.productId ?: "Choose below"}",
            )
            state.products?.records?.forEach { product ->
                HarvestCircleShellButton(
                    "${product.value.label} · ${product.id.takeLast(8)}",
                    { presenter.updateDraft { it.copy(productId = product.id) } },
                    Modifier.testTag("reference-product-${product.id}"),
                    primary = draft.productId == product.id,
                )
            }
            if (state.products?.more ==
                true
            ) {
                HarvestCircleShellButton("More products", { presenter.searchProducts(state.productSearch, next = true) })
            }
            HarvestCircleShellTextField(
                draft.nominalGrams,
                { text -> presenter.updateDraft { it.copy(nominalGrams = text) } },
                "Nominal grams per original pack (optional)",
                "Leave blank when unknown",
                inputModifier = Modifier.testTag("reference-grams"),
            )
        }
        if (draft.kind == HubReferenceKind.Location) {
            if (draft.recordId == null) {
                HarvestCircleShellMenuButton(
                    draft.locationPurpose,
                    HubLocationPurpose.entries.map { HarvestCircleMenuOption(it, it.label) },
                    { purpose -> presenter.updateDraft { it.copy(locationPurpose = purpose) } },
                    "Purpose: ${draft.locationPurpose.label}",
                    Modifier.testTag("reference-purpose"),
                )
            } else {
                HarvestCircleShellText("Purpose: ${draft.locationPurpose.label}. A changed purpose needs another location identity.")
            }
        }
        if (draft.kind == HubReferenceKind.Party) {
            HarvestCircleShellText(
                "Suppliers, producers, source issuers and recipients can be different parties. A name alone does not establish a source.",
            )
        }
        if (state.draftNeedsReview) {
            HarvestCircleShellButton(
                "Review current versions",
                presenter::reviewDraftContext,
                Modifier.testTag("reference-review-context"),
                enabled = !state.loading,
            )
        }
        state.contextReview?.let { review ->
            HarvestCircleShellText(
                review.current?.let { "Current label: ${it.value.label} · Version ${it.revision}" }
                    ?: "Current catalog loaded. Review your selections before retaining this input.",
            )
            HarvestCircleShellButton(
                "Use reviewed versions; retain input",
                presenter::acceptContextReview,
                Modifier.testTag("reference-accept-context"),
                enabled = !state.loading,
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellButton(
                "Save reference",
                submit,
                Modifier.testTag("reference-save"),
                primary = true,
                enabled = !state.loading && !state.draftNeedsReview,
            )
            HarvestCircleShellButton("Discard draft", presenter::closeDraft, Modifier.testTag("reference-discard"))
        }
        HarvestCircleShellText("Labels and units are checked when saved. Recorded versions remain in history.")
    }
}

@Composable
private fun ReferenceSubmissionPanel(
    state: ReferenceViewState,
    presenter: ReferencePresenter,
) {
    val submission = requireNotNull(state.submission)
    HarvestCircleShellPanel(Modifier.testTag("reference-submission")) {
        HarvestCircleShellText("Original operation: ${submission.attempt.operation.value}")
        HarvestCircleShellText("Submitted label: ${submission.attempt.edit.value.label}")
        when (submission) {
            is ReferenceSubmission.Pending -> HarvestCircleShellText("Recording reference…")
            is ReferenceSubmission.Recorded -> {
                HarvestCircleShellText(
                    "Reference recorded",
                    Modifier.testTag("reference-recorded"),
                    HarvestCircleShellTextRole.SectionTitle,
                )
                ReferenceSummary(submission.receipt.current)
                HarvestCircleShellText("Recording time: ${utcTime(submission.receipt.recordedAt.value)}")
                HarvestCircleShellButton(
                    "View current records",
                    presenter::finishRecorded,
                    Modifier.testTag("reference-done"),
                    primary = true,
                )
            }
            is ReferenceSubmission.Rejected -> {
                HarvestCircleShellText(submission.problem.safeMessage)
                HarvestCircleShellButton(
                    "Review retained input",
                    presenter::reviewRejected,
                    Modifier.testTag("reference-review"),
                    enabled = !state.loading,
                )
                HarvestCircleShellButton("Check original result", presenter::checkOriginal, enabled = !state.loading)
            }
            is ReferenceSubmission.Uncertain -> {
                HarvestCircleShellText(submission.message, Modifier.testTag("reference-uncertain"))
                HarvestCircleShellText("Keep the original operation. Checking reads its result and does not submit it again.")
                HarvestCircleShellButton(
                    "Check original result",
                    presenter::checkOriginal,
                    Modifier.testTag("reference-check-original"),
                    enabled = !state.loading,
                )
            }
        }
    }
}

@Composable
private fun ReferenceHistoryPanel(
    state: ReferenceViewState,
    presenter: ReferencePresenter,
) {
    val record = requireNotNull(state.inspecting)
    HarvestCircleShellButton("Back to references", presenter::closeInspection)
    HarvestCircleShellText("Reference history", role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellText("Record: ${record.id}")
    if (!state.loading && state.history.isEmpty()) HarvestCircleShellText("No recorded versions are visible.")
    state.history.forEach { receipt ->
        HarvestCircleShellPanel {
            ReferenceSummary(receipt.current)
            receipt.previous?.let { HarvestCircleShellText("Previous label: ${it.value.label}") }
            HarvestCircleShellText("Recording time: ${utcTime(receipt.recordedAt.value)}")
            HarvestCircleShellText("Recorded by: ${receipt.actor.value}")
            HarvestCircleShellText("Operation: ${receipt.operationId.value}")
        }
    }
    if (state.historyMore) HarvestCircleShellButton("More versions", presenter::moreHistory, enabled = !state.loading)
}

@Composable
private fun ReferenceSummary(record: HubReferenceRecord) {
    HarvestCircleShellText(record.value.label, role = HarvestCircleShellTextRole.BodyStrong)
    HarvestCircleShellText("${record.value.kind.label} · ${record.id.takeLast(8)} · Version ${record.revision}")
    record.value.productId?.let { HarvestCircleShellText("Product reference: $it") }
    if (record.value.kind == HubReferenceKind.Pack) {
        HarvestCircleShellText(record.value.nominalGrams?.let { "Nominal mass: $it grams per original pack" } ?: "Nominal mass unknown")
    }
    record.value.locationPurpose?.let { HarvestCircleShellText("Purpose: ${it.label}") }
}

private fun utcTime(seconds: Long): String =
    runCatching { Instant.fromEpochSeconds(seconds).toString() }
        .getOrElse { "Outside the displayable time range" }
