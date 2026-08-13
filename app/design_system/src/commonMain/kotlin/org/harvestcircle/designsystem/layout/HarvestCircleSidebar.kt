package org.harvestcircle.designsystem.layout

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.utility.HarvestCircleVerticalDivider
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Canonical source-list/sidebar surface for navigation and project browsers. */
@Composable
public fun HarvestCircleSidebar(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    HarvestCircleSurface(
        modifier = modifier.fillMaxHeight(),
        role = HarvestCircleSurfaceRole.Sunken,
    ) {
        Column(
            modifier = Modifier.padding(HarvestCircleTheme.foundation.spacing.sm),
            content = content,
        )
        HarvestCircleVerticalDivider(modifier = Modifier.align(Alignment.CenterEnd))
    }
}

/** Quiet uppercase section label used inside [HarvestCircleSidebar]. */
@Composable
public fun HarvestCircleSidebarSectionHeader(
    text: String,
    modifier: Modifier = Modifier,
) {
    HarvestCircleText(
        text = text.uppercase(),
        modifier =
            modifier.padding(
                start = HarvestCircleTheme.foundation.spacing.md,
                top = HarvestCircleTheme.foundation.spacing.lg,
                end = HarvestCircleTheme.foundation.spacing.md,
                bottom = HarvestCircleTheme.foundation.spacing.xs,
            ),
        role = HarvestCircleTextRole.LabelSmall,
        tone = HarvestCircleContentTone.Muted,
        maxLines = 1,
    )
}
