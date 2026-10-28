package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HubLocationPurpose
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubSourceCertainty
import org.harvestcircle.application.HubSourceKind
import org.harvestcircle.application.StockPresenter
import org.harvestcircle.application.StockViewState
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMenuButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
internal fun ReceiptEditor(
    state: StockViewState,
    presenter: StockPresenter,
    submit: () -> Unit,
) {
    val draft = requireNotNull(state.draft)
    HarvestCircleShellButton("Back to stock view; keep draft", presenter::hideReceipt, Modifier.testTag("receipt-keep-draft"))
    HarvestCircleShellPanel(Modifier.testTag("receipt-editor")) {
        HarvestCircleShellText("Receive whole original packs", role = HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText("Record packs actually received into your custody. A planned destination does not move them there.")
        ReferenceChoice(
            "Original pack",
            "receipt-variant",
            state.references,
            HubReferenceKind.Pack,
            draft.variantId,
            { id -> presenter.updateDraft { it.copy(variantId = id) } },
            optional = false,
            emptyLabel = "Choose an original pack",
        )
        HarvestCircleShellTextField(
            draft.quantity,
            { text -> presenter.updateDraft { it.copy(quantity = text) } },
            "Counted original packs",
            "Whole count",
            inputModifier = Modifier.testTag("receipt-quantity"),
        )
        ReferenceChoice(
            "Actually received at",
            "receipt-receiving",
            state.references,
            HubReferenceKind.Location,
            draft.receivingId,
            { id -> presenter.updateDraft { it.copy(receivingId = id) } },
            optional = false,
            emptyLabel = "Choose the Receiving area",
            purpose = HubLocationPurpose.Receiving,
        )
        ReferenceChoice(
            "Planned put-away destination",
            "receipt-planned",
            state.references,
            HubReferenceKind.Location,
            draft.plannedLocationId,
            { id -> presenter.updateDraft { it.copy(plannedLocationId = id) } },
            emptyLabel = "Not planned",
        )
        ReferenceChoice(
            "Supplier",
            "receipt-supplier",
            state.references,
            HubReferenceKind.Party,
            draft.supplierId,
            { id -> presenter.updateDraft { it.copy(supplierId = id) } },
        )
        ReferenceChoice(
            "Producer",
            "receipt-producer",
            state.references,
            HubReferenceKind.Party,
            draft.producerId,
            { id -> presenter.updateDraft { it.copy(producerId = id) } },
        )
        HarvestCircleShellText("Supplier, producer and source issuer are independent. Leave unknown facts unknown.")
        ReceiptSourceFields(state, presenter)
        HarvestCircleShellText("When did the receipt actually occur?")
        HarvestCircleShellMenuButton(
            draft.occurrenceKind,
            listOf(
                HarvestCircleMenuOption(HubOccurrenceKind.Unknown, "Time not established"),
                HarvestCircleMenuOption(HubOccurrenceKind.Known, "Exact time known"),
                HarvestCircleMenuOption(HubOccurrenceKind.Interval, "Within a known interval"),
            ),
            { kind -> presenter.updateDraft { it.copy(occurrenceKind = kind) } },
            when (draft.occurrenceKind) {
                HubOccurrenceKind.Unknown -> "Time not established"
                HubOccurrenceKind.Known -> "Exact time known"
                HubOccurrenceKind.Interval -> "Within a known interval"
            },
            Modifier.testTag("receipt-occurrence-kind"),
        )
        if (draft.occurrenceKind != HubOccurrenceKind.Unknown) {
            HarvestCircleShellTextField(
                draft.occurrenceStart,
                { text -> presenter.updateDraft { it.copy(occurrenceStart = text) } },
                if (draft.occurrenceKind == HubOccurrenceKind.Interval) "Earliest occurrence (UTC)" else "Occurrence (UTC)",
                "2026-10-28T09:30:00Z",
                inputModifier = Modifier.testTag("receipt-occurrence-start"),
            )
        }
        if (draft.occurrenceKind == HubOccurrenceKind.Interval) {
            HarvestCircleShellTextField(
                draft.occurrenceEnd,
                { text -> presenter.updateDraft { it.copy(occurrenceEnd = text) } },
                "Latest occurrence (UTC)",
                "2026-10-28T10:00:00Z",
                inputModifier = Modifier.testTag("receipt-occurrence-end"),
            )
        }
        HarvestCircleShellText("Recording time is captured separately. Unknown occurrence time is preserved.")
        HarvestCircleShellTextField(
            draft.observation,
            { text -> presenter.updateDraft { it.copy(observation = text) } },
            "Manual confirmation",
            "Describe what you counted and confirmed",
            inputModifier = Modifier.testTag("receipt-observation"),
        )
        HarvestCircleShellText(
            "Saving records this physical intake. Source uncertainty remains visible; recording is not an acceptance or release decision.",
        )
        if (state.needsReview) {
            HarvestCircleShellButton(
                "Review current context",
                presenter::reviewContext,
                Modifier.testTag("receipt-review-context"),
                enabled = !state.loading,
            )
        }
        state.review?.let { review ->
            fun label(id: String?): String =
                review.references
                    .find { it.id == id }
                    ?.value
                    ?.label ?: id ?: "Unknown"
            HarvestCircleShellText("Current pack: ${label(draft.variantId)} · Receiving: ${label(draft.receivingId)}")
            HarvestCircleShellText("Your selections, count and observations are retained. Review these current names before continuing.")
            HarvestCircleShellButton(
                "Use reviewed context; retain input",
                presenter::acceptReview,
                Modifier.testTag("receipt-accept-context"),
                enabled = !state.loading,
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellButton(
                "Record actual receipt",
                submit,
                Modifier.testTag("receipt-save"),
                primary = true,
                enabled = !state.loading && !state.needsReview,
            )
            HarvestCircleShellButton("Discard receipt draft", presenter::closeDraft, Modifier.testTag("receipt-discard"))
        }
    }
}

@Composable
private fun ReceiptSourceFields(
    state: StockViewState,
    presenter: StockPresenter,
) {
    val draft = requireNotNull(state.draft)
    HarvestCircleShellText("Source identification", role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellMenuButton(
        draft.sourceCertainty,
        listOf(
            HarvestCircleMenuOption(HubSourceCertainty.Unknown, "Source unknown"),
            HarvestCircleMenuOption(HubSourceCertainty.Possible, "Possible source"),
            HarvestCircleMenuOption(HubSourceCertainty.Established, "Source established"),
        ),
        { certainty -> presenter.updateDraft { it.copy(sourceCertainty = certainty) } },
        when (draft.sourceCertainty) {
            HubSourceCertainty.Unknown -> "Source unknown"
            HubSourceCertainty.Possible -> "Possible source"
            HubSourceCertainty.Established -> "Source established"
        },
        Modifier.testTag("receipt-source-certainty"),
    )
    if (draft.sourceCertainty == HubSourceCertainty.Unknown) return
    if (draft.existingSourceId != null) {
        HarvestCircleShellText("Selected existing source: ${draft.sourceCode}")
        HarvestCircleShellText(
            "Issuer: ${state.references
                .find { it.id == draft.issuerId }
                ?.value
                ?.label ?: "Unknown"}",
        )
        HarvestCircleShellText(draft.provenance)
        HarvestCircleShellButton(
            "Describe a different new source",
            { presenter.updateDraft { it.copy(existingSourceId = null) } },
            Modifier.testTag("receipt-new-source"),
        )
        return
    }
    HarvestCircleShellMenuButton(
        draft.sourceKind,
        listOf(
            HarvestCircleMenuOption(HubSourceKind.ProducerBatch, "Producer batch"),
            HarvestCircleMenuOption(HubSourceKind.SupplierReference, "Supplier reference"),
            HarvestCircleMenuOption(HubSourceKind.Other, "Other original reference"),
        ),
        { kind -> presenter.updateDraft { it.copy(sourceKind = kind) } },
        when (draft.sourceKind) {
            HubSourceKind.ProducerBatch -> "Producer batch"
            HubSourceKind.SupplierReference -> "Supplier reference"
            HubSourceKind.Other -> "Other original reference"
        },
        Modifier.testTag("receipt-source-kind"),
    )
    ReferenceChoice(
        "Source issuer",
        "receipt-issuer",
        state.references,
        HubReferenceKind.Party,
        draft.issuerId,
        { id -> presenter.updateDraft { it.copy(issuerId = id) } },
    )
    HarvestCircleShellTextField(
        draft.sourceCode,
        { text -> presenter.updateDraft { it.copy(sourceCode = text) } },
        "Original source code",
        "As shown on the original reference",
        inputModifier = Modifier.testTag("receipt-source-code"),
    )
    HarvestCircleShellTextField(
        draft.provenance,
        { text -> presenter.updateDraft { it.copy(provenance = text) } },
        "Source observation",
        "Where did this identification come from?",
        inputModifier = Modifier.testTag("receipt-source-provenance"),
    )
    HarvestCircleShellText("Equal codes do not establish one source. To reuse a known source, begin from its Stock record.")
}
