package org.harvestcircle.designsystem.component.utility

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
public fun HarvestCircleHorizontalDivider(
    modifier: Modifier = Modifier,
    strong: Boolean = false,
) {
    Box(
        modifier =
            modifier
                .fillMaxWidth()
                .height(HarvestCircleTheme.shell.dimensions.dividerWidth)
                .background(
                    if (strong) {
                        HarvestCircleTheme.foundation.colors.border.default
                    } else {
                        HarvestCircleTheme.foundation.colors.border.subtle
                    },
                ),
    )
}

@Composable
public fun HarvestCircleVerticalDivider(
    modifier: Modifier = Modifier,
    strong: Boolean = false,
) {
    Box(
        modifier =
            modifier
                .fillMaxHeight()
                .width(HarvestCircleTheme.shell.dimensions.dividerWidth)
                .background(
                    if (strong) {
                        HarvestCircleTheme.foundation.colors.border.default
                    } else {
                        HarvestCircleTheme.foundation.colors.border.subtle
                    },
                ),
    )
}
