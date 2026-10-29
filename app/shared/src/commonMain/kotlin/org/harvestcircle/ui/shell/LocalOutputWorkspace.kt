package org.harvestcircle.ui.shell

import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
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
import org.harvestcircle.application.LocalOutputAction
import org.harvestcircle.application.LocalOutputPresenter
import org.harvestcircle.application.LocalOutputState
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
internal fun LocalOutputWorkspace(
    state: LocalOutputState,
    presenter: LocalOutputPresenter,
) {
    val focus = remember { FocusRequester() }
    LaunchedEffect(state.loading, state.busy) {
        if (!state.loading && !state.busy) focus.requestFocus()
    }
    Column(
        Modifier
            .fillMaxSize()
            .focusRequester(focus)
            .focusable()
            .verticalScroll(rememberScrollState())
            .padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("local-output-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleShellText(state.title, role = HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText("Sample data only · Local output")
        HarvestCircleShellButton("Back to record", { presenter.dismiss() }, Modifier.testTag("local-output-back"), enabled = !state.busy)
        if (state.loading) HarvestCircleShellText("Preparing the selected document…")
        state.document?.let { document ->
            HarvestCircleShellPanel {
                HarvestCircleShellText("Captured document", role = HarvestCircleShellTextRole.BodyStrong)
                SelectionContainer {
                    Column {
                        HarvestCircleShellText(document.suggestedFilename, Modifier.testTag("local-output-filename"))
                        HarvestCircleShellText(document.identity, Modifier.testTag("local-output-identity"))
                    }
                }
                HarvestCircleShellText(
                    "Saving, printing and retrying use this same captured content. Return to the record to prepare a different snapshot or version.",
                )
            }
            HarvestCircleShellButton(
                "Save HTML…",
                { presenter.perform(LocalOutputAction.SaveHtml) },
                Modifier.testTag("local-output-save"),
                primary = true,
                enabled = !state.busy,
            )
            HarvestCircleShellButton(
                "Print…",
                { presenter.perform(LocalOutputAction.Print) },
                Modifier.testTag("local-output-print"),
                enabled = !state.busy && !state.printNeedsReview,
            )
        }
        if (state.busy) {
            HarvestCircleShellText("Waiting for this local output to finish. No physical stock event is submitted.")
            HarvestCircleShellButton("Cancel this output", presenter::cancel, Modifier.testTag("local-output-cancel"))
        }
        if (state.printNeedsReview) {
            HarvestCircleShellButton(
                "I checked the print queue",
                presenter::acknowledgePrintReview,
                Modifier.testTag("local-output-review"),
                enabled = !state.busy,
            )
        }
        state.message?.let { HarvestCircleShellText(it, Modifier.testTag("local-output-message")) }
        HarvestCircleShellText(
            "A saved file or print-system submission does not confirm physical printing, external delivery, recipient acknowledgement or complete off-device history.",
        )
    }
}
