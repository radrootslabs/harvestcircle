package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HubReturnTiming
import org.harvestcircle.application.HubTraceSnapshot
import org.harvestcircle.application.TraceCountSection
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.application.TraceViewState
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
internal fun TraceCountInterpretations(
    snapshot: HubTraceSnapshot,
    state: TraceViewState,
    presenter: TracePresenter,
) {
    val counts = snapshot.countInterpretations
    if (counts?.accounts.isNullOrEmpty() && snapshot.countReturnAssociations.isEmpty() && !snapshot.countReturnUncertain) return
    HarvestCircleShellText("Count shortage interpretations", role = HarvestCircleShellTextRole.SectionTitle)
    HarvestCircleShellText(
        "Original count shortages stay recorded. Later explanations describe missing stock; returned stock retains its own population and handling.",
    )
    if (snapshot.countReturnUncertain) HarvestCircleShellText("Return association remains uncertain and needs review.")
    counts?.let {
        it.accounts.drop(state.countAccountsOffset).take(64).forEach { account ->
            HarvestCircleShellPanel(Modifier.testTag("trace-count-account-${account.countOperationId.value}")) {
                HarvestCircleShellText(
                    "Original missing ${account.grossShortageWholePacks} · Explained ${account.explainedWholePacks} · Unexplained ${account.unexplainedWholePacks}",
                    role = HarvestCircleShellTextRole.BodyStrong,
                )
                HarvestCircleShellText("Count confirmed ${stockUtc(account.confirmedOnHandAt.value)} · Group ${account.anchorGroupId}")
                HarvestCircleShellText("Reconciliation ${account.reconciliationId}")
                SelectionContainer { HarvestCircleShellText("Original count operation ${account.countOperationId.value}") }
            }
        }
        CountTracePager("count accounts", "accounts", state.countAccountsOffset, it.accounts.size) { delta ->
            presenter.countInterpretationsPage(TraceCountSection.Accounts, delta)
        }
        HarvestCircleShellText("Original and current explanations", role = HarvestCircleShellTextRole.SectionTitle)
        it.explanations.drop(state.countExplanationsOffset).take(64).forEach { explanation ->
            HarvestCircleShellPanel(Modifier.testTag("trace-count-explanation-${explanation.operationId.value}")) {
                val meaning = explanation.interpretation
                HarvestCircleShellText(
                    "Original ${meaning.originalWholePacks} · Current ${meaning.effectiveWholePacks} · Established returned ${explanation.establishedReturnWholePacks}",
                    role = HarvestCircleShellTextRole.BodyStrong,
                )
                HarvestCircleShellText("Original recipient: ${explanation.originalRecipient.value.label}")
                HarvestCircleShellText("Current recipient: ${meaning.recipient?.value?.label ?: "Unknown recipient"}")
                HarvestCircleShellText("Explained departure: ${actualOccurrence(explanation.occurrence)}")
                HarvestCircleShellText("Recorded ${stockUtc(explanation.recordedAt.value)} · ${explanation.actor.value}")
                SelectionContainer {
                    HarvestCircleShellText(
                        "Explanation ${explanation.operationId.value} · Count ${explanation.countOperationId.value}",
                    )
                }
            }
        }
        CountTracePager("explanations", "explanations", state.countExplanationsOffset, it.explanations.size) { delta ->
            presenter.countInterpretationsPage(TraceCountSection.Explanations, delta)
        }
        val classifications = it.accounts.flatMap { account -> account.classifications }
        if (classifications.isNotEmpty()) {
            HarvestCircleShellText("Retained quantity classifications", role = HarvestCircleShellTextRole.SectionTitle)
            classifications.drop(state.countClassificationsOffset).take(64).forEach { classification ->
                HarvestCircleShellPanel(Modifier.testTag("trace-count-classification-${classification.operationId.value}")) {
                    HarvestCircleShellText(
                        "${classification.beforeWholePacks} → ${classification.afterWholePacks} · Revision ${classification.stockRevision}",
                    )
                    SelectionContainer {
                        HarvestCircleShellText(
                            "Classification ${classification.operationId.value} · Explanation ${classification.explanationOperationId.value}",
                        )
                    }
                }
            }
            CountTracePager("classifications", "classifications", state.countClassificationsOffset, classifications.size) { delta ->
                presenter.countInterpretationsPage(TraceCountSection.Classifications, delta)
            }
        }
    }
    if (snapshot.countReturnAssociations.isNotEmpty()) {
        HarvestCircleShellText("Later Return associations", role = HarvestCircleShellTextRole.SectionTitle)
        snapshot.countReturnAssociations.drop(state.countAssociationsOffset).take(64).forEach { association ->
            HarvestCircleShellPanel(Modifier.testTag("trace-count-association-${association.operationId.value}")) {
                HarvestCircleShellText("${association.wholePacks} whole returned packs", role = HarvestCircleShellTextRole.BodyStrong)
                HarvestCircleShellText(
                    when (association.timing) {
                        HubReturnTiming.AfterOrSame -> "Return timing is consistent with the explained departure"
                        HubReturnTiming.Uncertain -> "Return timing remains uncertain"
                        HubReturnTiming.Contradictory -> "Return timing conflicts and needs review"
                    },
                )
                SelectionContainer {
                    HarvestCircleShellText(
                        "Return ${association.returnOperationId.value} · Explanation ${association.explanationOperationId.value}",
                    )
                }
                SelectionContainer {
                    HarvestCircleShellText(
                        "Returned population ${association.returnedPopulationId} · Explanation population ${association.explanationTraceId}",
                    )
                }
                HarvestCircleShellText("Explained departure: ${actualOccurrence(association.explanationOccurrence)}")
                HarvestCircleShellText("Return occurrence: ${actualOccurrence(association.returnOccurrence)}")
                HarvestCircleShellText("Association recorded ${stockUtc(association.recordedAt.value)} · ${association.actor.value}")
            }
        }
        CountTracePager("Return associations", "returns", state.countAssociationsOffset, snapshot.countReturnAssociations.size) { delta ->
            presenter.countInterpretationsPage(TraceCountSection.ReturnAssociations, delta)
        }
    }
}

@Composable
private fun CountTracePager(
    label: String,
    tag: String,
    offset: Int,
    count: Int,
    page: (Int) -> Unit,
) {
    if (count <= 64) return
    Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
        HarvestCircleShellButton("Previous $label", { page(-1) }, Modifier.testTag("trace-count-$tag-previous"), enabled = offset > 0)
        HarvestCircleShellButton("Next $label", { page(1) }, Modifier.testTag("trace-count-$tag-next"), enabled = offset + 64 < count)
    }
}
