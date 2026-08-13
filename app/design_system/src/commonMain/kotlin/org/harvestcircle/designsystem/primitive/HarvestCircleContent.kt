package org.harvestcircle.designsystem.primitive

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.graphics.Color

/** Internal content-color propagation used by HarvestCircle primitives and components. */
internal val LocalHarvestCircleContentColor = compositionLocalOf { Color.Unspecified }

@Composable
internal fun ProvideHarvestCircleContentColor(
    color: Color,
    content: @Composable () -> Unit,
) {
    CompositionLocalProvider(
        LocalHarvestCircleContentColor provides color,
        content = content,
    )
}

@Composable
internal fun currentHarvestCircleContentColor(): Color {
    val inherited = LocalHarvestCircleContentColor.current
    return if (inherited == Color.Unspecified) {
        org.harvestcircle.designsystem.theme.HarvestCircleTheme.foundation.colors.content.primary
    } else {
        inherited
    }
}
