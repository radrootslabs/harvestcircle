@file:OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)

package org.harvestcircle.designsystem.component.feedback

import androidx.compose.foundation.BasicTooltipBox
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberBasicTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.PopupPositionProvider
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Compact AppKit-style tooltip triggered by pointer hover or touch long press. */
@Composable
public fun HarvestCircleTooltip(
    text: String,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val state = rememberBasicTooltipState(isPersistent = true)
    val density = LocalDensity.current
    val gapPx = with(density) { 6.dp.roundToPx() }
    val positionProvider =
        remember(gapPx) {
            object : PopupPositionProvider {
                override fun calculatePosition(
                    anchorBounds: IntRect,
                    windowSize: IntSize,
                    layoutDirection: LayoutDirection,
                    popupContentSize: IntSize,
                ): IntOffset {
                    val centeredX =
                        anchorBounds.left + (anchorBounds.width - popupContentSize.width) / 2
                    val clampedX =
                        centeredX.coerceIn(
                            minimumValue = 0,
                            maximumValue =
                                (windowSize.width - popupContentSize.width).coerceAtLeast(0),
                        )
                    val above = anchorBounds.top - popupContentSize.height - gapPx
                    val y =
                        if (above >= 0) {
                            above
                        } else {
                            (anchorBounds.bottom + gapPx)
                                .coerceAtMost((windowSize.height - popupContentSize.height).coerceAtLeast(0))
                        }
                    return IntOffset(clampedX, y)
                }
            }
        }

    val container =
        if (HarvestCircleTheme.foundation.colors.isDark) {
            HarvestCircleTheme.foundation.colors.surface.raised
        } else {
            HarvestCircleTheme.foundation.colors.content.primary
        }
    val contentColor =
        if (HarvestCircleTheme.foundation.colors.isDark) {
            HarvestCircleTheme.foundation.colors.content.primary
        } else {
            HarvestCircleTheme.foundation.colors.content.inverse
        }

    BasicTooltipBox(
        positionProvider = positionProvider,
        tooltip = {
            HarvestCircleSurface(
                role = HarvestCircleSurfaceRole.Overlay,
                shape = HarvestCircleTheme.foundation.shapes.control,
                border = BorderStroke(1.dp, HarvestCircleTheme.foundation.colors.border.strong),
                shadowElevation = HarvestCircleTheme.component.elevations.overlay,
                color = container,
                contentColor = contentColor,
            ) {
                ProvideHarvestCircleContentColor(contentColor) {
                    HarvestCircleText(
                        text = text,
                        modifier =
                            Modifier.padding(
                                horizontal = HarvestCircleTheme.foundation.spacing.md,
                                vertical = HarvestCircleTheme.foundation.spacing.xs,
                            ),
                        role = HarvestCircleTextRole.BodySmall,
                        tone = HarvestCircleContentTone.Inherit,
                        maxLines = 3,
                    )
                }
            }
        },
        state = state,
        modifier = modifier,
        focusable = false,
        enableUserInput = true,
        content = content,
    )
}
