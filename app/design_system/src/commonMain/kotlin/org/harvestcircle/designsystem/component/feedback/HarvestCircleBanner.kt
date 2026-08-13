package org.harvestcircle.designsystem.component.feedback

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleIconSize
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import org.harvestcircle.designsystem.primitive.HarvestCircleIcon
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.harvestcircle.designsystem.theme.color.HarvestCircleFeedbackRoleColors
import org.jetbrains.compose.resources.DrawableResource

public enum class HarvestCircleBannerTone {
    Info,
    Success,
    Warning,
    Error,
}

@Composable
private fun roleColors(tone: HarvestCircleBannerTone): HarvestCircleFeedbackRoleColors =
    when (tone) {
        HarvestCircleBannerTone.Info -> HarvestCircleTheme.foundation.colors.feedback.info
        HarvestCircleBannerTone.Success -> HarvestCircleTheme.foundation.colors.feedback.success
        HarvestCircleBannerTone.Warning -> HarvestCircleTheme.foundation.colors.feedback.warning
        HarvestCircleBannerTone.Error -> HarvestCircleTheme.foundation.colors.feedback.error
    }

private fun defaultIcon(tone: HarvestCircleBannerTone): DrawableResource =
    when (tone) {
        HarvestCircleBannerTone.Info -> HarvestCircleIcons.Info
        HarvestCircleBannerTone.Success -> HarvestCircleIcons.Check
        HarvestCircleBannerTone.Warning,
        HarvestCircleBannerTone.Error,
        -> HarvestCircleIcons.Warning
    }

/** Compact inline status well modeled after canonical macOS informational callouts. */
@Composable
public fun HarvestCircleBanner(
    message: String,
    modifier: Modifier = Modifier,
    tone: HarvestCircleBannerTone = HarvestCircleBannerTone.Info,
    title: String? = null,
    icon: DrawableResource = defaultIcon(tone),
    action: (@Composable RowScope.() -> Unit)? = null,
) {
    val colors = roleColors(tone)

    HarvestCircleSurface(
        modifier = modifier,
        role = HarvestCircleSurfaceRole.Base,
        shape = HarvestCircleTheme.foundation.shapes.card,
        border = BorderStroke(1.dp, colors.border),
        color = colors.subtle,
        contentColor = colors.onSubtle,
    ) {
        ProvideHarvestCircleContentColor(colors.onSubtle) {
            Row(
                modifier = Modifier.padding(HarvestCircleTheme.foundation.spacing.lg),
                horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                HarvestCircleIcon(
                    resource = icon,
                    contentDescription = null,
                    size = HarvestCircleIconSize.Medium,
                    tone = HarvestCircleContentTone.Inherit,
                )

                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.xs),
                ) {
                    if (title != null) {
                        HarvestCircleText(
                            text = title,
                            role = HarvestCircleTextRole.BodyStrong,
                            tone = HarvestCircleContentTone.Inherit,
                        )
                    }
                    HarvestCircleText(
                        text = message,
                        role = HarvestCircleTextRole.Body,
                        tone = HarvestCircleContentTone.Inherit,
                    )
                }

                action?.invoke(this)
            }
        }
    }
}
