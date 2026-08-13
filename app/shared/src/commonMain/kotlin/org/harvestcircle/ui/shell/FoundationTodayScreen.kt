package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.action.HarvestCircleLabeledButton
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBadge
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

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
            verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
        ) {
            HarvestCircleBadge(model.context, Modifier.testTag("today-context"))
            HarvestCircleText("No active commitments", role = HarvestCircleTextRole.SectionTitle)
            HarvestCircleText("Explore nearby buying circles or open a shared Nostr reference.")
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                HarvestCircleLabeledButton(
                    label = "Explore circles",
                    accessibilityLabel = "Explore circles",
                    modifier = Modifier.testTag("today-explore-circles"),
                    enabled = false,
                    onClick = {},
                )
                HarvestCircleLabeledButton(
                    label = "Open a Nostr reference",
                    accessibilityLabel = "Open a Nostr reference",
                    modifier =
                        Modifier
                            .shellFocusTarget(ShellFocusTarget.TodayReference)
                            .testTag("today-open-reference"),
                    onClick = openNostrReference,
                )
            }
            HarvestCircleText(
                "Not available in this build.",
                Modifier.testTag("today-deferred-helper"),
                HarvestCircleTextRole.BodySmall,
                HarvestCircleContentTone.Secondary,
            )
        }
    }
}
