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
import org.harvestcircle.application.CountContext
import org.harvestcircle.application.CountFields
import org.harvestcircle.application.CountLookupKind
import org.harvestcircle.application.CountPresenter
import org.harvestcircle.application.CountResult
import org.harvestcircle.application.CountSubmission
import org.harvestcircle.application.CountViewState
import org.harvestcircle.application.HubCountBlockReason
import org.harvestcircle.application.HubCountDifference
import org.harvestcircle.application.HubCountObservation
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubSourceCertainty
import org.harvestcircle.application.HubSourceKind
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMenuButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Presents native observations and comparisons without deriving stock consequences. */
@Composable
fun CountWorkspace(
    state: CountViewState,
    presenter: CountPresenter,
    submit: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("count-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleShellText("Sample data only · Count observations are separate from stock adjustments")
        state.message?.let { HarvestCircleShellText(it, Modifier.testTag("count-message")) }
        if (state.loading) HarvestCircleShellText("Reading native count context…")
        val submission = state.submission
        if (state.originalLookup != null) {
            OriginalCountLookup(state, presenter)
        } else if (submission != null) {
            CountOutcome(submission, state.loading, presenter)
        } else {
            HarvestCircleShellButton("Back to stock; keep draft", presenter::park, Modifier.testTag("count-park"))
            if (state.needsReview && state.draft == null) {
                HarvestCircleShellButton(
                    "Read current context",
                    presenter::reviewContext,
                    Modifier.testTag("count-retry-context"),
                    enabled = !state.loading,
                )
            }
            state.draft?.let { draft ->
                CountContextSummary(draft.context, "Captured context")
                if (state.needsReview) {
                    HarvestCircleShellText("Review current context before continuing. Saved input is retained.")
                    HarvestCircleShellButton("Read current context", presenter::reviewContext, enabled = !state.loading)
                    state.review?.let {
                        CountContextSummary(it, "Current context")
                        HarvestCircleShellButton(
                            "Use reviewed current context",
                            presenter::acceptReview,
                            Modifier.testTag("count-accept-review"),
                            enabled = !state.loading,
                        )
                    }
                }
                CountEditor(draft.context, draft.fields, presenter)
                HarvestCircleShellButton(
                    if (draft.fields.confirmed) {
                        "Confirmation recorded; clear"
                    } else {
                        "I confirm these observed facts and the displayed consequences"
                    },
                    { presenter.confirm(!draft.fields.confirmed) },
                    Modifier.testTag("count-confirm"),
                    enabled = !state.loading && !state.needsReview && (draft.context !is CountContext.Review || draft.context.review.ready),
                )
                HarvestCircleShellButton(
                    when (draft.context) {
                        is CountContext.Opening -> "Record counted opening"
                        is CountContext.Observe -> "Save observation only"
                        is CountContext.Review -> "Post reviewed reconciliation"
                    },
                    submit,
                    Modifier.testTag("count-submit"),
                    primary = true,
                    enabled = !state.loading && !state.needsReview && draft.fields.confirmed,
                )
                HarvestCircleShellButton("Discard unsubmitted draft", presenter::discard, enabled = !state.loading)
            }
        }
    }
}

@Composable
private fun CountContextSummary(
    context: CountContext,
    title: String,
) {
    HarvestCircleShellPanel {
        HarvestCircleShellText(title, role = HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText("Stock version ${context.stockRevision} · Catalog version ${context.catalogRevision}")
        when (context) {
            is CountContext.Opening ->
                HarvestCircleShellText(
                    "Count present stock. Earlier intake history and source remain explicitly unknown unless established separately.",
                )
            is CountContext.Observe -> {
                HarvestCircleShellText("Group ${context.group.group.id} · ${context.group.group.wholePacks} recorded original packs")
                HarvestCircleShellText(
                    "${context.group.product.value.label} · ${context.group.variant.value.label} · ${context.group.location.value.label}",
                )
                HarvestCircleShellText("Physical identification: ${context.group.group.identification ?: "Not established"}")
                HarvestCircleShellText("Saving this observation changes neither stock nor work.")
            }
            is CountContext.Review -> {
                val review = context.review
                HarvestCircleShellText("Original observation: ${review.observation.id}")
                HarvestCircleShellText(
                    "${review.observation.product.value.label} · ${review.observation.variant.value.label} · ${review.observation.location.value.label}",
                )
                HarvestCircleShellText(
                    "Observed by ${review.observation.actor.value} · Recorded ${stockUtc(review.observation.recordedAt.value)}",
                )
                HarvestCircleShellText("Original note: ${review.observation.observation}")
                HarvestCircleShellText(
                    "Current group ${review.current.id} · Pack ${review.current.variantId} · Location ${review.current.locationId}",
                )
                HarvestCircleShellText("Current physical identification: ${review.current.identification ?: "Not established"}")
                HarvestCircleShellText(
                    "Recorded then ${review.observation.recordedWholePacks} · Counted ${review.observation.countedWholePacks} · Current ${review.current.wholePacks}",
                )
                HarvestCircleShellText(
                    "Count cutoff: ${review.observation.cutoff.kind} · ${review.observation.cutoff.startUnixSeconds?.let(
                        ::stockUtc,
                    ) ?: "Unknown"}",
                )
                HarvestCircleShellText(
                    when (val difference = review.difference) {
                        HubCountDifference.NoVariance -> "No count variance"
                        is HubCountDifference.Shortage -> "Shortage: ${difference.wholePacks} original packs"
                        is HubCountDifference.Surplus -> "Separate surplus: ${difference.wholePacks} original packs"
                    },
                )
                HarvestCircleShellText("Native blockers: ${review.blockers.joinToString { countBlockLabel(it) }.ifEmpty { "None" }}")
                review.current.work.forEach { scope ->
                    val work = scope.requirement.quantity
                    HarvestCircleShellText("Current work ${scope.requirement.id} · ${scope.progress}")
                    HarvestCircleShellText(
                        "Required ${work.required} · Completed ${work.completed} · No longer required ${work.noLongerRequired} · Remaining ${work.remaining}",
                    )
                }
                if (review.difference is HubCountDifference.Shortage) {
                    HarvestCircleShellText(
                        "Posting removes this scoped shortage and retires only its pending work. Completed and previously retired work, and original receipts, remain history.",
                    )
                }

                HarvestCircleShellText(
                    "Count time does not establish when or why stock was lost. Original receipts and completed work remain history.",
                )
                if (review.difference is HubCountDifference.Surplus) {
                    HarvestCircleShellText(
                        "Surplus requires independently observed physical identification. It creates separate unknown-source, review-required stock without inherited acceptance, holds or work.",
                    )
                }
            }
        }
    }
}

@Composable
private fun CountEditor(
    context: CountContext,
    fields: CountFields,
    presenter: CountPresenter,
) {
    if (context is CountContext.Opening) {
        ReferenceChoice(
            "Original pack",
            "count-variant",
            context.references,
            HubReferenceKind.Pack,
            fields.variantId,
            { id -> presenter.edit { it.copy(variantId = id) } },
            optional = false,
        )
        ReferenceChoice(
            "Counted at",
            "count-location",
            context.references,
            HubReferenceKind.Location,
            fields.locationId,
            { id -> presenter.edit { it.copy(locationId = id) } },
            optional = false,
        )
        CountField("Earlier intake reference (blank means unknown)", "count-history", fields.historyReference) { text ->
            presenter.edit {
                it.copy(historyReference = text)
            }
        }
        HarvestCircleShellMenuButton(
            fields.sourceCertainty,
            HubSourceCertainty.entries.map { HarvestCircleMenuOption(it, it.name) },
            { certainty -> presenter.edit { it.copy(sourceCertainty = certainty) } },
            fields.sourceCertainty.name,
            Modifier.testTag("count-source-certainty"),
        )
        if (fields.sourceCertainty != HubSourceCertainty.Unknown) {
            HarvestCircleShellMenuButton(
                fields.sourceKind,
                HubSourceKind.entries.map { HarvestCircleMenuOption(it, it.name) },
                { kind -> presenter.edit { it.copy(sourceKind = kind) } },
                fields.sourceKind.name,
                Modifier.testTag("count-source-kind"),
            )
            ReferenceChoice(
                "Source issuer",
                "count-issuer",
                context.references,
                HubReferenceKind.Party,
                fields.issuerId,
                { id -> presenter.edit { it.copy(issuerId = id) } },
            )
            CountField(
                "Original source code",
                "count-source-code",
                fields.sourceCode,
            ) { text -> presenter.edit { it.copy(sourceCode = text) } }
            CountField("Source identification provenance", "count-source-provenance", fields.sourceProvenance) { text ->
                presenter.edit {
                    it.copy(sourceProvenance = text)
                }
            }
        }
        HarvestCircleShellText(
            "Source knowledge is separate from earlier intake history. Opening creates no historical supplier or receipt.",
        )
    }
    if (context !is CountContext.Review) {
        CountField(
            "Counted whole original packs",
            "count-quantity",
            fields.quantity,
        ) { text -> presenter.edit { it.copy(quantity = text) } }
        HarvestCircleShellMenuButton(
            fields.cutoffKind,
            HubOccurrenceKind.entries.map { HarvestCircleMenuOption(it, it.name) },
            { kind -> presenter.edit { it.copy(cutoffKind = kind) } },
            fields.cutoffKind.name,
            Modifier.testTag("count-cutoff-kind"),
        )
        if (fields.cutoffKind != HubOccurrenceKind.Unknown) {
            CountField("Count cutoff (UTC)", "count-cutoff", fields.cutoffStart) { text -> presenter.edit { it.copy(cutoffStart = text) } }
            HarvestCircleShellButton("Counted now", presenter::countedNow)
        }
        if (fields.cutoffKind == HubOccurrenceKind.Interval) {
            CountField("Latest cutoff (UTC)", "count-cutoff-end", fields.cutoffEnd) { text -> presenter.edit { it.copy(cutoffEnd = text) } }
        }
        HarvestCircleShellText("Opening requires a known count time. Uncertain observation cutoffs remain evidence and block posting.")
    }
    if (context is CountContext.Opening || context is CountContext.Review && context.review.difference is HubCountDifference.Surplus) {
        CountField(
            "Distinct physical identifier",
            "count-label",
            fields.physicalLabel,
        ) { text -> presenter.edit { it.copy(physicalLabel = text) } }
        CountField(
            "How this stock is physically distinguished",
            "count-identification",
            fields.physicalObservation,
        ) { text ->
            presenter.edit {
                it.copy(physicalObservation = text)
            }
        }
    }
    CountField(
        if (context is CountContext.Review) "Reason for reconciliation" else "Count observation",
        "count-observation",
        fields.observation,
    ) { text ->
        presenter.edit {
            it.copy(observation = text)
        }
    }
}

@Composable
private fun CountField(
    label: String,
    tag: String,
    value: String,
    change: (String) -> Unit,
) {
    HarvestCircleShellTextField(value, change, label, "", inputModifier = Modifier.testTag(tag))
}

@Composable
private fun CountOutcome(
    submission: CountSubmission,
    loading: Boolean,
    presenter: CountPresenter,
) {
    SelectionContainer { HarvestCircleShellText("Original operation: ${submission.attempt.operation.value}") }
    when (submission) {
        is CountSubmission.Pending -> HarvestCircleShellText("Recording; retain the original operation…")
        is CountSubmission.Uncertain -> {
            HarvestCircleShellText(submission.message, Modifier.testTag("count-uncertain"))
            HarvestCircleShellText("Read-only lookup cannot create another operation. Absence after restore does not prove no effect.")
            HarvestCircleShellButton(
                "Check original result",
                presenter::checkOriginal,
                Modifier.testTag("count-check-original"),
                enabled = !loading,
            )
        }
        is CountSubmission.Rejected -> {
            HarvestCircleShellText(submission.problem.safeMessage)
            HarvestCircleShellButton("Review retained input", presenter::reviewRejected, enabled = !loading)
            HarvestCircleShellButton("Check original result", presenter::checkOriginal, enabled = !loading)
        }
        is CountSubmission.Recorded -> {
            HarvestCircleShellText("Original result recorded", Modifier.testTag("count-recorded"))
            CountResultSummary(submission.result, presenter::reviewRecordedObservation)
            HarvestCircleShellButton("View current stock", presenter::finishRecorded, Modifier.testTag("count-done"))
        }
    }
}

private fun countBlockLabel(reason: HubCountBlockReason): String =
    when (reason) {
        HubCountBlockReason.ScopeChanged -> "Physical scope changed"
        HubCountBlockReason.NewerReconciliation -> "A newer reconciliation exists"
        HubCountBlockReason.NotLatestObservation -> "A newer observation exists"
        HubCountBlockReason.CutoffUncertain -> "Count cutoff is uncertain"
        HubCountBlockReason.CutoffPrecedesRecordedActivity -> "Recorded activity follows this cutoff"
        HubCountBlockReason.UnresolvedActual -> "An actual report needs review"
        HubCountBlockReason.UncertainPriorOutcome -> "A prior operation is unconfirmed"
    }

@Composable
private fun OriginalCountLookup(
    state: CountViewState,
    presenter: CountPresenter,
) {
    val lookup = requireNotNull(state.originalLookup)
    HarvestCircleShellText("Read an original count result", Modifier.testTag("count-original-lookup"))
    HarvestCircleShellText(
        "Use the original operation ID retained before closing. Lookup is read-only; missing history cannot authorize retry.",
    )
    HarvestCircleShellMenuButton(
        lookup.kind,
        CountLookupKind.entries.map { HarvestCircleMenuOption(it, it.name) },
        { kind -> presenter.editOriginalLookup(kind, lookup.operationText) },
        lookup.kind.name,
        Modifier.testTag("count-lookup-kind"),
    )
    CountField("Original operation ID", "count-lookup-operation", lookup.operationText) {
        presenter.editOriginalLookup(lookup.kind, it)
    }
    HarvestCircleShellButton(
        "Read original result",
        presenter::lookupOriginal,
        Modifier.testTag("count-lookup-read"),
        enabled = !state.loading,
    )
    lookup.result?.let { result ->
        HarvestCircleShellText("Original result found", Modifier.testTag("count-lookup-result"))
        SelectionContainer { HarvestCircleShellText("Original operation: ${result.operation.value}") }
        CountResultSummary(result, presenter::reviewLookedUpObservation)
    }
    HarvestCircleShellButton("Back to stock", presenter::discard, Modifier.testTag("count-lookup-close"), enabled = !state.loading)
}

@Composable
private fun CountResultSummary(
    result: CountResult,
    reviewObservation: (() -> Unit)? = null,
) {
    when (result) {
        is CountResult.Opened -> {
            HarvestCircleShellText("As-created acceptance: ${result.receipt.original.acceptance}")
            StockPopulationSummary(result.receipt.original.population)
            HarvestCircleShellText("Original group ${result.receipt.original.group.id} · ${result.receipt.original.group.identification}")
        }
        is CountResult.Observed -> {
            OriginalCountObservationSummary(result.observation)
            HarvestCircleShellText(
                "Saved observation ${result.observation.countedWholePacks}; original recorded count ${result.observation.recordedWholePacks}. Stock and work were unchanged.",
            )
            reviewObservation?.let { review ->
                HarvestCircleShellButton("Review reconciliation separately", review, Modifier.testTag("count-review-post"))
            }
        }
        is CountResult.Posted -> {
            OriginalCountObservationSummary(result.receipt.observation)
            HarvestCircleShellText(
                "Posting references: ${result.receipt.product.value.label} · ${result.receipt.variant.value.label} · ${result.receipt.location.value.label}",
            )
            HarvestCircleShellText("Reconciled by ${result.receipt.actor.value} · Recorded ${stockUtc(result.receipt.recordedAt.value)}")
            HarvestCircleShellText("On-hand confirmed ${stockUtc(result.receipt.confirmedOnHandAt.value)}")
            HarvestCircleShellText("Original reconciliation reason: ${result.receipt.reason}")
            HarvestCircleShellText(
                "Original before ${result.receipt.before.wholePacks} · On hand after ${result.receipt.onHandAfter}",
            )
            result.receipt.workChanges.forEach {
                val before = it.before.quantity
                HarvestCircleShellText("Before work ${it.before.id} · Required ${before.required} · Completed ${before.completed}")
                HarvestCircleShellText("Before no longer required ${before.noLongerRequired} · Remaining ${before.remaining}")
                HarvestCircleShellText(
                    "Work ${it.after.id} · Required ${it.after.quantity.required} · Completed ${it.after.quantity.completed}",
                )
                HarvestCircleShellText(
                    "No longer required ${it.after.quantity.noLongerRequired} · Remaining ${it.after.quantity.remaining}",
                )
            }
            result.receipt.surplus?.let {
                HarvestCircleShellText("As-created surplus acceptance: ${it.acceptance}")
                StockPopulationSummary(it.population)
            }
        }
    }
}

@Composable
private fun OriginalCountObservationSummary(observation: HubCountObservation) {
    HarvestCircleShellText(
        "Original physical fact: ${observation.product.value.label} · ${observation.variant.value.label} · ${observation.location.value.label}",
    )
    HarvestCircleShellText("Original group ${observation.groupId} v${observation.groupRevision} · Population ${observation.populationId}")
    HarvestCircleShellText("Observed by ${observation.actor.value} · Recorded ${stockUtc(observation.recordedAt.value)}")
    HarvestCircleShellText(
        "Original cutoff: ${observation.cutoff.kind} · ${observation.cutoff.startUnixSeconds?.let(
            ::stockUtc,
        ) ?: "Unknown"} · ${observation.cutoff.endUnixSeconds?.let(::stockUtc) ?: "No interval end"}",
    )
    HarvestCircleShellText("Original observation: ${observation.observation}")
}
