package org.harvestcircle.designsystem.layout

import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.BoxWithConstraintsScope
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/** Layout class based on available width rather than platform identity. */
public enum class HarvestCircleWindowWidthClass {
    Narrow,
    Medium,
    Wide,
}

/**
 * Classifies the local constraints and passes the result to [content].
 *
 * Breakpoints are product-neutral and intentionally based on available space, not desktop/web.
 */
@Composable
public fun HarvestCircleResponsive(
    modifier: Modifier = Modifier,
    content: @Composable BoxWithConstraintsScope.(HarvestCircleWindowWidthClass) -> Unit,
) {
    BoxWithConstraints(modifier = modifier) {
        val widthClass =
            when {
                maxWidth < 720.dp -> HarvestCircleWindowWidthClass.Narrow
                maxWidth < 1120.dp -> HarvestCircleWindowWidthClass.Medium
                else -> HarvestCircleWindowWidthClass.Wide
            }

        content(widthClass)
    }
}
