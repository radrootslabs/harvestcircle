package org.harvestcircle.designsystem.primitive

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.focus.HarvestCircleFocusDismissBehavior
import org.harvestcircle.designsystem.internal.focus.harvestCircleClearFocusOnBackgroundPress
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Semantic surface roles. */
public enum class HarvestCircleSurfaceRole {
    Canvas,
    Base,
    Raised,
    Sunken,
    Selected,
    Overlay,
}

@Composable
internal fun harvestCircleSurfaceColor(role: HarvestCircleSurfaceRole): Color =
    when (role) {
        HarvestCircleSurfaceRole.Canvas -> HarvestCircleTheme.foundation.colors.surface.canvas
        HarvestCircleSurfaceRole.Base -> HarvestCircleTheme.foundation.colors.surface.base
        HarvestCircleSurfaceRole.Raised -> HarvestCircleTheme.foundation.colors.surface.raised
        HarvestCircleSurfaceRole.Sunken -> HarvestCircleTheme.foundation.colors.surface.sunken
        HarvestCircleSurfaceRole.Selected -> HarvestCircleTheme.foundation.colors.surface.selected
        HarvestCircleSurfaceRole.Overlay -> HarvestCircleTheme.foundation.colors.surface.overlay
    }

/** Flat semantic surface with optional AppKit-style border and restrained shadow. */
@Composable
public fun HarvestCircleSurface(
    modifier: Modifier = Modifier,
    role: HarvestCircleSurfaceRole = HarvestCircleSurfaceRole.Base,
    shape: Shape = RectangleShape,
    border: BorderStroke? = null,
    shadowElevation: Dp = 0.dp,
    color: Color? = null,
    contentColor: Color? = null,
    content: @Composable BoxScope.() -> Unit,
) {
    val surface = color ?: harvestCircleSurfaceColor(role)
    val resolvedContentColor = contentColor ?: HarvestCircleTheme.foundation.colors.content.primary
    val decoratedModifier =
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
            .background(color = surface, shape = shape)
            .then(
                if (border != null) {
                    Modifier.border(border = border, shape = shape)
                } else {
                    Modifier
                },
            )

    ProvideHarvestCircleContentColor(resolvedContentColor) {
        Box(
            modifier = decoratedModifier,
            content = content,
        )
    }
}

/**
 * Root application surface.
 *
 * By default, pressing non-interactive canvas space clears the current Compose focus. This matches
 * conventional desktop and browser behavior where a focus ring disappears after the user clicks
 * away from a control. Set [focusDismissBehavior] to [HarvestCircleFocusDismissBehavior.KeepFocused] for an
 * editor canvas or another surface that intentionally retains focus. Set [forceFocusDismissal] only
 * when the product must override a component that deliberately captured focus.
 */
@Composable
public fun HarvestCircleAppSurface(
    modifier: Modifier = Modifier,
    focusDismissBehavior: HarvestCircleFocusDismissBehavior =
        HarvestCircleFocusDismissBehavior.ClearOnBackgroundPress,
    forceFocusDismissal: Boolean = false,
    content: @Composable BoxScope.() -> Unit,
) {
    val focusManager = LocalFocusManager.current

    HarvestCircleSurface(
        modifier =
            modifier
                .fillMaxSize()
                .harvestCircleClearFocusOnBackgroundPress(
                    focusManager = focusManager,
                    enabled =
                        focusDismissBehavior ==
                            HarvestCircleFocusDismissBehavior.ClearOnBackgroundPress,
                    force = forceFocusDismissal,
                ),
        role = HarvestCircleSurfaceRole.Canvas,
        shape = RectangleShape,
        content = content,
    )
}
