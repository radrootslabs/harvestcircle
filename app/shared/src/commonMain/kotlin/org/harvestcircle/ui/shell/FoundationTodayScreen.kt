package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp

data class FoundationTodayModel(
    val context: String,
)

@Composable
fun FoundationTodayScreen(
    model: FoundationTodayModel,
    openNostrReference: () -> Unit,
) {
    SingleFocusTemplate {
        Column(
            Modifier.testTag("foundation-today"),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            ShellBadge(model.context, Modifier.testTag("today-context"))
            ShellText("No active commitments", textRole = ShellTextRole.SectionTitle)
            ShellText("Explore nearby buying circles or open a shared Nostr reference.")
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                ShellAction(
                    "Explore circles",
                    "Explore circles",
                    "today-explore-circles",
                    enabled = false,
                    onClick = {},
                )
                ShellAction(
                    "Open a Nostr reference",
                    "Open a Nostr reference",
                    "today-open-reference",
                    onClick = openNostrReference,
                )
            }
            ShellText("Not available in this build.", Modifier.testTag("today-deferred-helper"), ShellTextRole.Secondary)
        }
    }
}
