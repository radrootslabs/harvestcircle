package org.harvestcircle.designsystem.layout

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import org.harvestcircle.designsystem.component.utility.HarvestCircleHorizontalDivider
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Flat unified-toolbar surface with a single bottom separator, matching canonical macOS windows. */
@Composable
public fun HarvestCircleToolbar(
    modifier: Modifier = Modifier,
    content: @Composable RowScope.() -> Unit,
) {
    Column(
        modifier =
            modifier
                .fillMaxWidth()
                .background(HarvestCircleTheme.foundation.colors.surface.raised),
    ) {
        ProvideHarvestCircleContentColor(HarvestCircleTheme.foundation.colors.content.primary) {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .height(HarvestCircleTheme.shell.dimensions.toolbarHeight)
                        .padding(horizontal = HarvestCircleTheme.shell.layout.paneInset),
                horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                content = content,
            )
        }
        HarvestCircleHorizontalDivider()
    }
}
