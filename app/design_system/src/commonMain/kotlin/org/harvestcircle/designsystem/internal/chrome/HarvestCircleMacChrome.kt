package org.harvestcircle.designsystem.internal.chrome

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/**
 * Produces the restrained vertical control gradient used by AppKit before Liquid Glass.
 *
 * The gradient is deliberately subtle: it should read as a native bezel, not as glossy skeuomorphism.
 */
@Composable
internal fun harvestCircleMacControlBrush(
    base: Color,
    emphasized: Boolean = false,
): Brush {
    if (base.alpha == 0f) return SolidColor(Color.Transparent)

    val dark = HarvestCircleTheme.foundation.colors.isDark
    val top =
        when {
            dark && emphasized -> lerp(base, Color.White, 0.12f)
            dark -> lerp(base, Color.White, 0.07f)
            emphasized -> lerp(base, Color.White, 0.14f)
            else -> lerp(base, Color.White, 0.42f)
        }
    val bottom =
        when {
            dark && emphasized -> lerp(base, Color.Black, 0.06f)
            dark -> lerp(base, Color.Black, 0.04f)
            emphasized -> lerp(base, Color.Black, 0.08f)
            else -> lerp(base, Color.Black, 0.035f)
        }

    return Brush.verticalGradient(
        colors = listOf(top, base, bottom),
    )
}

/** Border color for a filled control, derived without exposing physical palette tokens. */
internal fun harvestCircleMacFilledBorder(
    container: Color,
    dark: Boolean,
): Color =
    if (dark) {
        lerp(container, Color.White, 0.18f)
    } else {
        lerp(container, Color.Black, 0.20f)
    }

/**
 * A softer filled-control contour for primary actions that already have a strong blue fill.
 *
 * Standard-contrast themes blend toward white so the bezel remains visible without looking like a
 * second focus ring. High-contrast themes intentionally keep the stronger canonical boundary.
 */
internal fun harvestCircleMacSoftFilledBorder(
    container: Color,
    dark: Boolean,
    highContrast: Boolean,
): Color =
    if (highContrast) {
        harvestCircleMacFilledBorder(container = container, dark = dark)
    } else {
        lerp(container, Color.White, if (dark) 0.24f else 0.18f)
    }

/**
 * Optionally reserves stable space for and draws a macOS-style keyboard focus ring around [content].
 *
 * Public controls default [focusRing] to [HarvestCircleFocusRing.WhenFocused]. The layout reserves
 * the complete ring width plus the clear gap regardless of focus state, so focus changes never shift
 * content. [ringShape] is the mathematically expanded outer contour; it must not reuse the inner
 * [shape] on a larger rectangle or the two corner curves will not remain parallel.
 */
@Composable
internal fun HarvestCircleMacFocusFrame(
    focused: Boolean,
    focusRing: HarvestCircleFocusRing,
    modifier: Modifier = Modifier,
    shape: Shape = HarvestCircleTheme.foundation.shapes.control,
    ringShape: Shape = shape,
    contentAlignment: Alignment = Alignment.Center,
    content: @Composable BoxScope.() -> Unit,
) {
    if (focusRing == HarvestCircleFocusRing.None) {
        Box(
            modifier = modifier,
            contentAlignment = contentAlignment,
            content = content,
        )
        return
    }

    val frameInset = HarvestCircleTheme.shell.dimensions.focusRingWidth + HarvestCircleTheme.shell.dimensions.focusRingGap

    Box(
        modifier =
            modifier
                .border(
                    width = HarvestCircleTheme.shell.dimensions.focusRingWidth,
                    color = if (focused) HarvestCircleTheme.foundation.colors.focus.ring else Color.Transparent,
                    shape = ringShape,
                ).padding(frameInset),
        contentAlignment = contentAlignment,
        content = content,
    )
}

/** Draws a flat AppKit-style bezel with optional restrained shadow. */
@Composable
internal fun HarvestCircleMacBezel(
    modifier: Modifier = Modifier,
    brush: Brush,
    border: BorderStroke,
    shape: Shape = HarvestCircleTheme.foundation.shapes.control,
    shadowElevation: Dp = 0.dp,
    contentAlignment: Alignment = Alignment.Center,
    content: @Composable BoxScope.() -> Unit,
) {
    Box(
        modifier =
            modifier
                .then(
                    if (shadowElevation > 0.dp) {
                        Modifier.shadow(
                            elevation = shadowElevation,
                            shape = shape,
                            clip = false,
                        )
                    } else {
                        Modifier
                    },
                ).clip(shape)
                .background(brush = brush, shape = shape)
                .border(border = border, shape = shape),
        contentAlignment = contentAlignment,
        content = content,
    )
}
