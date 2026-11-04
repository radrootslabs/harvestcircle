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
import org.harvestcircle.application.CustodyLookupKind
import org.harvestcircle.application.CustodyPresenter
import org.harvestcircle.application.CustodyResult
import org.harvestcircle.application.CustodySubmission
import org.harvestcircle.application.CustodyTarget
import org.harvestcircle.application.CustodyViewState
import org.harvestcircle.application.HubReturnResolution
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellMenuButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
fun CustodyWorkspace(
    state: CustodyViewState,
    presenter: CustodyPresenter,
    submit: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("custody-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleShellText("Sample data only · Returned custody and observed physical removal")
        state.message?.let { HarvestCircleShellText(it, Modifier.testTag("custody-message")) }
        if (state.loading) HarvestCircleShellText("Reading native custody context…")
        val lookup = state.originalLookup
        val submission = state.submission
        if (lookup != null) {
            HarvestCircleShellButton("Back to stock", presenter::park, enabled = !state.loading)
            HarvestCircleShellMenuButton(
                lookup.kind,
                CustodyLookupKind.entries.map { HarvestCircleMenuOption(it, it.name) },
                {
                    presenter.editOriginalLookup(it, lookup.operationText)
                },
                lookup.kind.name,
                Modifier.testTag("custody-lookup-kind"),
            )
            CustodyField(
                "Original operation ID",
                "custody-lookup-operation",
                lookup.operationText,
            ) { presenter.editOriginalLookup(lookup.kind, it) }
            HarvestCircleShellButton(
                "Read original result",
                presenter::lookupOriginal,
                Modifier.testTag("custody-lookup-submit"),
                enabled = !state.loading,
            )
            HarvestCircleShellText(
                "This only reads the original. An absent result does not establish no effect or permit another submission.",
            )
            lookup.result?.let { CustodyResultSummary(it) }
            HarvestCircleShellButton("Close original lookup", presenter::discard, enabled = !state.loading)
        } else if (submission != null) {
            SelectionContainer {
                HarvestCircleShellText("Original operation ${submission.attempt.operation.value}", Modifier.testTag("custody-operation"))
            }
            when (submission) {
                is CustodySubmission.Pending -> HarvestCircleShellText("Waiting for the original native outcome…")
                is CustodySubmission.Recorded -> {
                    CustodyResultSummary(submission.result)
                    HarvestCircleShellButton(
                        "Done; read current stock",
                        presenter::finishRecorded,
                        Modifier.testTag("custody-done"),
                        primary = true,
                    )
                }
                is CustodySubmission.Uncertain -> {
                    HarvestCircleShellText(submission.message)
                    HarvestCircleShellText("Keep this original operation. Do not record the same physical facts under a new ID.")
                    HarvestCircleShellButton(
                        "Check original result",
                        presenter::checkOriginal,
                        Modifier.testTag("custody-check-original"),
                        enabled = !state.loading,
                    )
                }
                is CustodySubmission.Rejected -> {
                    HarvestCircleShellText(submission.problem.safeMessage)
                    HarvestCircleShellButton(
                        "Read current context before a corrected submission",
                        presenter::reviewRejected,
                        Modifier.testTag("custody-review-rejected"),
                        enabled = !state.loading,
                    )
                    HarvestCircleShellButton("Check original result", presenter::checkOriginal, enabled = !state.loading)
                }
            }
        } else {
            HarvestCircleShellButton("Back to stock; keep draft", presenter::park, Modifier.testTag("custody-park"))
            val draft = state.draft
            if (draft == null) {
                HarvestCircleShellButton(
                    "Read current context",
                    presenter::refresh,
                    Modifier.testTag("custody-retry-context"),
                    enabled = !state.loading,
                )
            } else {
                HarvestCircleShellText(
                    if (state.target ==
                        CustodyTarget.Return
                    ) {
                        "Record actual returned stock"
                    } else {
                        "Record actual physical disposition"
                    },
                    role = HarvestCircleShellTextRole.SectionTitle,
                )
                HarvestCircleShellText("Stock version ${draft.context.stockRevision} · Catalog version ${draft.context.catalogRevision}")
                if (state.needsReview) HarvestCircleShellText("Review current native context before confirming. Your input is retained.")
                HarvestCircleShellButton(
                    "Read current context",
                    presenter::refresh,
                    Modifier.testTag("custody-refresh"),
                    enabled = !state.loading,
                )
                state.review?.let { review ->
                    HarvestCircleShellPanel {
                        HarvestCircleShellText("Current stock ${review.stockRevision} · Catalog ${review.catalogRevision}")
                        review.group?.let {
                            HarvestCircleShellText(
                                "${it.group.wholePacks} whole packs · ${it.location.value.label} · Group version ${it.group.revision}",
                            )
                        }
                        HarvestCircleShellButton(
                            "Use this reviewed context",
                            presenter::acceptReview,
                            Modifier.testTag("custody-accept-context"),
                            enabled = !state.loading,
                        )
                    }
                }
                CustodyEditor(requireNotNull(state.target), draft, presenter)
                HarvestCircleShellButton(
                    if (draft.fields.common.confirmed) "Confirmation recorded; clear" else "I confirm these actual physical facts",
                    { presenter.confirm(!draft.fields.common.confirmed) },
                    Modifier.testTag("custody-confirm"),
                    enabled =
                        !state.loading && !state.needsReview,
                )
                HarvestCircleShellButton(
                    if (state.target ==
                        CustodyTarget.Return
                    ) {
                        "Record returned custody"
                    } else {
                        "Record observed disposition"
                    },
                    submit,
                    Modifier.testTag("custody-submit"),
                    primary = true,
                    enabled =
                        !state.loading && !state.needsReview && draft.fields.common.confirmed,
                )
                HarvestCircleShellButton(
                    "Discard unsubmitted draft",
                    presenter::discard,
                    Modifier.testTag("custody-discard"),
                    enabled = !state.loading,
                )
            }
        }
    }
}

@Composable
internal fun CustodyField(
    label: String,
    tag: String,
    value: String,
    changed: (String) -> Unit,
) {
    HarvestCircleShellTextField(value, changed, label, label, inputModifier = Modifier.testTag(tag))
}

@Composable
private fun CustodyResultSummary(result: CustodyResult) {
    HarvestCircleShellPanel(Modifier.testTag("custody-result")) {
        when (result) {
            is CustodyResult.Returned -> {
                val receipt = result.receipt
                StockPopulationSummary(receipt.population)
                HarvestCircleShellText("Recorded returned custody · ${receipt.population.originalWholePacks} whole packs")
                HarvestCircleShellText("Returning party: ${receipt.returnedBy?.value?.label ?: "Unknown"}")
                HarvestCircleShellText(
                    "Sample acceptance: ${receipt.acceptance} · New intake work: ${receipt.intakeWork?.quantity?.remaining ?: "None"}",
                )
                HarvestCircleShellText(
                    when (val resolution = receipt.resolution) {
                        HubReturnResolution.Unknown -> "Original departure: Unknown"
                        HubReturnResolution.Unrecorded -> "Claimed original departure: Not recorded here at this intake"
                        HubReturnResolution.EstablishedScope -> "Association established for the explicitly selected original group only"
                        is HubReturnResolution.Unresolved -> "Original group association unresolved: ${resolution.reason}"
                    },
                )
                receipt.claim?.let {
                    HarvestCircleShellText(
                        "Claimed departure ${it.departureId} · Group ${it.originalGroupId ?: "Unknown"}",
                    )
                }
                receipt.departure?.let { HarvestCircleShellText("Original recipient: ${it.recipient.value.label}") }
                HarvestCircleShellText("Return/departure timing: ${receipt.timing}. Earlier source, acceptance and holds are not inferred.")
                receipt.plausibleSources.forEach { source ->
                    HarvestCircleShellText(
                        "Original candidate: ${source.original.identification?.label ?: "No separate identifier"} · ${source.original.variant.value.label} · ${source.original.location.value.label}",
                    )
                    HarvestCircleShellText(
                        "Group ${source.original.groupId} · ${source.original.wholePacks} departed · ${source.previouslyEstablishedReturnWholePacks} previously associated",
                    )
                }
                receipt.claimedSource?.let {
                    HarvestCircleShellText(
                        "Original group ${it.original.groupId}: ${it.original.wholePacks} departed packs · ${it.previouslyEstablishedReturnWholePacks} previously associated returns",
                    )
                }
                receipt.currentAllowances?.let { capture ->
                    HarvestCircleShellText("Return context recorded at stock version ${capture.stockRevision}")
                    capture.lines.forEach { line ->
                        HarvestCircleShellText(
                            "Group ${line.originalGroupId}: effective departure ${line.effectiveDepartureWholePacks} packs · Original physical debit bound ${line.originalDebitReturnBoundWholePacks} packs",
                        )
                        line.quantityHead?.let {
                            HarvestCircleShellText("Quantity amendment ${it.operationId.value} · Stock version ${it.stockRevision}")
                        }
                        if (line.unresolvedReturns) HarvestCircleShellText("Potentially related earlier returns remain unresolved.")
                    }
                }
            }
            is CustodyResult.Disposed -> {
                val receipt = result.receipt
                HarvestCircleShellText("Recorded physical disposition", role = HarvestCircleShellTextRole.SectionTitle)
                HarvestCircleShellText(
                    "${receipt.before.wholePacks} whole packs · ${receipt.product.value.label} · ${receipt.variant.value.label} · ${receipt.location.value.label}",
                )
                HarvestCircleShellText(
                    if (receipt.kind ==
                        org.harvestcircle.application.HubDispositionKind.Destroyed
                    ) {
                        "Destroyed"
                    } else {
                        "Other observed physical removal"
                    },
                )
                HarvestCircleShellText(receipt.observation)
                HarvestCircleShellText(
                    "This is separate from buyer departure, unexplained count loss or a refund. Hold history remains retained.",
                )
                HarvestCircleShellText(
                    "Original group ${receipt.before.id} · ${receipt.before.identification?.label ?: "No separate physical identifier"}",
                )
                receipt.before.identification?.let { HarvestCircleShellText(it.observation) }
                HarvestCircleShellText("Recorded ${stockUtc(receipt.recordedAt.value)} · ${receipt.actor.value}")
                HarvestCircleShellText("Actual occurrence: ${receipt.occurrence.kind}")
                receipt.occurrence.startUnixSeconds?.let { HarvestCircleShellText("Start: ${stockUtc(it)}") }
                receipt.occurrence.endUnixSeconds?.let { HarvestCircleShellText("End: ${stockUtc(it)}") }
                receipt.before.work.forEach { allocation ->
                    HarvestCircleShellText("Retained group work: ${allocation.progress} · ${allocation.requirement.id}")
                    val original = allocation.requirement.quantity
                    HarvestCircleShellText(
                        "Original global counters: required ${original.required} · completed ${original.completed} · retired ${original.noLongerRequired} · pending ${original.remaining}",
                    )
                }
                receipt.workChanges.forEach {
                    HarvestCircleShellText(
                        "Work ${it.before.id}: ${it.retiredWholePacks} pending packs retired; completed ${it.after.quantity.completed}",
                    )
                }
            }
        }
        SelectionContainer { HarvestCircleShellText("Original operation ${result.operation.value}") }
    }
}
