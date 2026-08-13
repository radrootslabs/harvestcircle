package org.harvestcircle.designsystem.primitive

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.unit.Dp
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleIconSize
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.jetbrains.compose.resources.DrawableResource
import org.jetbrains.compose.resources.painterResource

@Composable
private fun iconColor(tone: HarvestCircleContentTone): Color =
    when (tone) {
        HarvestCircleContentTone.Primary -> HarvestCircleTheme.foundation.colors.content.primary
        HarvestCircleContentTone.Secondary -> HarvestCircleTheme.foundation.colors.content.secondary
        HarvestCircleContentTone.Muted -> HarvestCircleTheme.foundation.colors.content.muted
        HarvestCircleContentTone.Disabled -> HarvestCircleTheme.foundation.colors.content.disabled
        HarvestCircleContentTone.Inverse -> HarvestCircleTheme.foundation.colors.content.inverse
        HarvestCircleContentTone.Inherit -> currentHarvestCircleContentColor()
    }

@Composable
private fun iconSize(size: HarvestCircleIconSize): Dp =
    when (size) {
        HarvestCircleIconSize.Small -> HarvestCircleTheme.shell.dimensions.iconSmall
        HarvestCircleIconSize.Medium -> HarvestCircleTheme.shell.dimensions.iconMedium
        HarvestCircleIconSize.Large -> HarvestCircleTheme.shell.dimensions.iconLarge
    }

/** Canonical resource-backed icon rendered with Foundation rather than Material Icon. */
@Composable
public fun HarvestCircleIcon(
    resource: DrawableResource,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    size: HarvestCircleIconSize = HarvestCircleIconSize.Medium,
    tone: HarvestCircleContentTone = HarvestCircleContentTone.Inherit,
    tint: Color? = null,
) {
    Image(
        painter = painterResource(resource),
        contentDescription = contentDescription,
        modifier = modifier.size(iconSize(size)),
        colorFilter = ColorFilter.tint(tint ?: iconColor(tone)),
    )
}
