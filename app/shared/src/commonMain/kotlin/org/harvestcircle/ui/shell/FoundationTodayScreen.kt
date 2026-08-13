package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellEmptyState

data class FoundationTodayModel(
    val context: String,
)

@Composable
fun FoundationTodayScreen(
    model: FoundationTodayModel,
    openNostrReference: () -> Unit,
) {
    HarvestCircleShellEmptyState(
        title = "No active commitments",
        body =
            listOf(
                "Explore nearby buying circles or open a shared Nostr reference.",
                "Not available in this build.",
            ),
        modifier = Modifier.testTag("foundation-today"),
        context = model.context,
        contextModifier = Modifier.testTag("today-context"),
    ) {
        HarvestCircleShellButton(
            text = "Explore circles",
            onClick = {},
            modifier = Modifier.testTag("today-explore-circles"),
            enabled = false,
        )
        HarvestCircleShellButton(
            text = "Open a Nostr reference",
            onClick = openNostrReference,
            modifier =
                Modifier
                    .shellFocusTarget(ShellFocusTarget.TodayReference)
                    .testTag("today-open-reference"),
            primary = true,
        )
    }
}
