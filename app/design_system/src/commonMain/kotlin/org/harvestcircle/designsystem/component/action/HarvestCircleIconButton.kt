package org.harvestcircle.designsystem.component.action

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleControlSize
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.component.HarvestCircleIconSize
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacBezel
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacFocusFrame
import org.harvestcircle.designsystem.internal.chrome.harvestCircleMacControlBrush
import org.harvestcircle.designsystem.internal.interaction.harvestCircleHoverable
import org.harvestcircle.designsystem.internal.interaction.harvestCircleInteractions
import org.harvestcircle.designsystem.internal.interaction.rememberHarvestCircleInteractionSources
import org.harvestcircle.designsystem.primitive.HarvestCircleIcon
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.jetbrains.compose.resources.DrawableResource

@Composable
private fun iconButtonVisualSize(size: HarvestCircleControlSize): Dp =
    when (size) {
        HarvestCircleControlSize.Small -> HarvestCircleTheme.shell.dimensions.controlSmall
        HarvestCircleControlSize.Medium -> HarvestCircleTheme.shell.dimensions.controlMedium
        HarvestCircleControlSize.Large -> HarvestCircleTheme.shell.dimensions.controlLarge
    }

/** Borderless AppKit toolbar action with stable hover, press, focus, and required accessible label. */
@Composable
public fun HarvestCircleIconButton(
    onClick: () -> Unit,
    icon: DrawableResource,
    label: String,
    modifier: Modifier = Modifier,
    size: HarvestCircleControlSize = HarvestCircleControlSize.Medium,
    enabled: Boolean = true,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
) {
    val sources = rememberHarvestCircleInteractionSources()
    val interactions = sources.harvestCircleInteractions(enabled)
    val ghost = HarvestCircleTheme.foundation.colors.action.ghost
    val visualSize = iconButtonVisualSize(size)
    val container =
        when {
            !enabled -> Color.Transparent
            interactions.pressed -> ghost.pressed
            interactions.hovered -> ghost.hover
            else -> Color.Transparent
        }
    val contentColor = if (enabled) ghost.content else ghost.disabledContent
    val border =
        if (interactions.hovered || interactions.pressed) {
            HarvestCircleTheme.foundation.colors.border.subtle
        } else {
            Color.Transparent
        }

    Box(
        modifier =
            modifier
                .defaultMinSize(
                    minWidth = HarvestCircleTheme.shell.dimensions.minimumInteractive,
                    minHeight = HarvestCircleTheme.shell.dimensions.minimumInteractive,
                ).semantics { contentDescription = label }
                .harvestCircleHoverable(sources = sources, enabled = enabled)
                .clickable(
                    interactionSource = sources.activationSource,
                    indication = null,
                    enabled = enabled,
                    role = Role.Button,
                    onClick = onClick,
                ),
        contentAlignment = Alignment.Center,
    ) {
        HarvestCircleMacFocusFrame(
            focused = interactions.focused,
            focusRing = focusRing,
            shape = HarvestCircleTheme.foundation.shapes.control,
            ringShape = HarvestCircleTheme.foundation.shapes.controlFocusRing,
        ) {
            HarvestCircleMacBezel(
                modifier = Modifier.size(visualSize),
                brush = harvestCircleMacControlBrush(container),
                border = BorderStroke(1.dp, border),
                shape = HarvestCircleTheme.foundation.shapes.control,
            ) {
                ProvideHarvestCircleContentColor(contentColor) {
                    HarvestCircleIcon(
                        resource = icon,
                        contentDescription = null,
                        size =
                            when (size) {
                                HarvestCircleControlSize.Small -> HarvestCircleIconSize.Small
                                HarvestCircleControlSize.Medium -> HarvestCircleIconSize.Medium
                                HarvestCircleControlSize.Large -> HarvestCircleIconSize.Large
                            },
                        tone = if (enabled) HarvestCircleContentTone.Inherit else HarvestCircleContentTone.Disabled,
                    )
                }
            }
        }
    }
}
