package org.harvestcircle.designsystem.component.feedback

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Compact, non-interactive status label. */
@Composable
public fun HarvestCircleBadge(
    label: String,
    modifier: Modifier = Modifier,
) {
    HarvestCircleSurface(
        modifier = modifier.semantics(mergeDescendants = true) {},
        role = HarvestCircleSurfaceRole.Base,
        shape = HarvestCircleTheme.foundation.shapes.control,
        border =
            BorderStroke(
                HarvestCircleTheme.shell.dimensions.dividerWidth,
                HarvestCircleTheme.foundation.colors.border.default,
            ),
    ) {
        HarvestCircleText(
            text = label,
            modifier =
                Modifier.padding(
                    horizontal = HarvestCircleTheme.foundation.spacing.md,
                    vertical = HarvestCircleTheme.foundation.spacing.xs,
                ),
            role = HarvestCircleTextRole.LabelSmall,
            tone = HarvestCircleContentTone.Secondary,
        )
    }
}
