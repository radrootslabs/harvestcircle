package org.harvestcircle.designsystem.component.selection

import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.component.HarvestCircleIconSize
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacFocusFrame
import org.harvestcircle.designsystem.internal.chrome.harvestCircleMacControlBrush
import org.harvestcircle.designsystem.internal.interaction.harvestCircleHoverable
import org.harvestcircle.designsystem.internal.interaction.harvestCircleInteractions
import org.harvestcircle.designsystem.internal.interaction.rememberHarvestCircleInteractionSources
import org.harvestcircle.designsystem.primitive.HarvestCircleIcon
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

private val CheckboxShape = RoundedCornerShape(3.dp)
private val CheckboxFocusRingShape = RoundedCornerShape(7.dp)

@Composable
private fun HarvestCircleCheckboxVisual(
    checked: Boolean,
    enabled: Boolean,
    hovered: Boolean,
    focused: Boolean,
    focusRing: HarvestCircleFocusRing,
) {
    val colors = HarvestCircleTheme.foundation.colors
    val container =
        when {
            !enabled && checked -> colors.action.primary.disabled
            !enabled -> colors.surface.sunken
            checked -> colors.action.primary.rest
            hovered -> colors.action.ghost.hover
            else -> colors.surface.raised
        }
    val border =
        when {
            !enabled -> colors.border.subtle
            checked -> lerp(colors.action.primary.rest, Color.Black, 0.16f)
            hovered -> colors.border.strong
            else -> colors.border.default
        }

    HarvestCircleMacFocusFrame(
        focused = focused,
        focusRing = focusRing,
        shape = CheckboxShape,
        ringShape = CheckboxFocusRingShape,
    ) {
        Box(
            modifier =
                Modifier
                    .size(HarvestCircleTheme.shell.dimensions.selectionControl)
                    .clip(CheckboxShape)
                    .background(
                        brush = harvestCircleMacControlBrush(container, emphasized = checked),
                        shape = CheckboxShape,
                    ).border(BorderStroke(1.dp, border), CheckboxShape),
            contentAlignment = Alignment.Center,
        ) {
            if (checked) {
                ProvideHarvestCircleContentColor(
                    if (enabled) colors.action.primary.content else colors.content.disabled,
                ) {
                    HarvestCircleIcon(
                        resource = HarvestCircleIcons.Check,
                        contentDescription = null,
                        size = HarvestCircleIconSize.Small,
                        tone = HarvestCircleContentTone.Inherit,
                    )
                }
            }
        }
    }
}

/** Canonical AppKit checkbox with an optional merged, clickable label. */
@Composable
public fun HarvestCircleCheckbox(
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    label: String? = null,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
) {
    val sources = rememberHarvestCircleInteractionSources()
    val interactions = sources.harvestCircleInteractions(enabled)
    val toggleModifier =
        modifier
            .defaultMinSize(
                minWidth = HarvestCircleTheme.shell.dimensions.minimumInteractive,
                minHeight = HarvestCircleTheme.shell.dimensions.minimumInteractive,
            ).harvestCircleHoverable(sources = sources, enabled = enabled)
            .toggleable(
                value = checked,
                interactionSource = sources.activationSource,
                indication = null,
                enabled = enabled,
                role = Role.Checkbox,
                onValueChange = onCheckedChange,
            )

    if (label == null) {
        Box(
            modifier = toggleModifier,
            contentAlignment = Alignment.Center,
        ) {
            HarvestCircleCheckboxVisual(
                checked = checked,
                enabled = enabled,
                hovered = interactions.hovered,
                focused = interactions.focused,
                focusRing = focusRing,
            )
        }
    } else {
        Row(
            modifier = toggleModifier,
            horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.sm),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            HarvestCircleCheckboxVisual(
                checked = checked,
                enabled = enabled,
                hovered = interactions.hovered,
                focused = interactions.focused,
                focusRing = focusRing,
            )
            HarvestCircleText(
                text = label,
                role = HarvestCircleTextRole.Body,
                tone = if (enabled) HarvestCircleContentTone.Primary else HarvestCircleContentTone.Disabled,
            )
        }
    }
}

@Composable
private fun HarvestCircleRadioVisual(
    selected: Boolean,
    enabled: Boolean,
    hovered: Boolean,
    focused: Boolean,
    focusRing: HarvestCircleFocusRing,
) {
    val colors = HarvestCircleTheme.foundation.colors
    val container =
        when {
            !enabled && selected -> colors.action.primary.disabled
            !enabled -> colors.surface.sunken
            selected -> colors.action.primary.rest
            hovered -> colors.action.ghost.hover
            else -> colors.surface.raised
        }
    val border =
        when {
            !enabled -> colors.border.subtle
            selected -> lerp(colors.action.primary.rest, Color.Black, 0.16f)
            hovered -> colors.border.strong
            else -> colors.border.default
        }

    HarvestCircleMacFocusFrame(
        focused = focused,
        focusRing = focusRing,
        shape = CircleShape,
        ringShape = CircleShape,
    ) {
        Box(
            modifier =
                Modifier
                    .size(HarvestCircleTheme.shell.dimensions.selectionControl)
                    .clip(CircleShape)
                    .background(
                        brush = harvestCircleMacControlBrush(container, emphasized = selected),
                        shape = CircleShape,
                    ).border(BorderStroke(1.dp, border), CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            if (selected) {
                Box(
                    modifier =
                        Modifier
                            .size(HarvestCircleTheme.shell.dimensions.selectionControl * 0.38f)
                            .clip(CircleShape)
                            .background(
                                if (enabled) {
                                    colors.action.primary.content
                                } else {
                                    colors.content.disabled
                                },
                            ),
                )
            }
        }
    }
}

/** Canonical AppKit radio button with an optional merged, clickable label. */
@Composable
public fun HarvestCircleRadioButton(
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    label: String? = null,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
) {
    val sources = rememberHarvestCircleInteractionSources()
    val interactions = sources.harvestCircleInteractions(enabled)
    val selectableModifier =
        modifier
            .defaultMinSize(
                minWidth = HarvestCircleTheme.shell.dimensions.minimumInteractive,
                minHeight = HarvestCircleTheme.shell.dimensions.minimumInteractive,
            ).harvestCircleHoverable(sources = sources, enabled = enabled)
            .selectable(
                selected = selected,
                interactionSource = sources.activationSource,
                indication = null,
                enabled = enabled,
                role = Role.RadioButton,
                onClick = onClick,
            )

    if (label == null) {
        Box(
            modifier = selectableModifier,
            contentAlignment = Alignment.Center,
        ) {
            HarvestCircleRadioVisual(
                selected = selected,
                enabled = enabled,
                hovered = interactions.hovered,
                focused = interactions.focused,
                focusRing = focusRing,
            )
        }
    } else {
        Row(
            modifier = selectableModifier,
            horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.sm),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            HarvestCircleRadioVisual(
                selected = selected,
                enabled = enabled,
                hovered = interactions.hovered,
                focused = interactions.focused,
                focusRing = focusRing,
            )
            HarvestCircleText(
                text = label,
                role = HarvestCircleTextRole.Body,
                tone = if (enabled) HarvestCircleContentTone.Primary else HarvestCircleContentTone.Disabled,
            )
        }
    }
}

@Composable
private fun HarvestCircleSwitchVisual(
    checked: Boolean,
    enabled: Boolean,
    hovered: Boolean,
    focused: Boolean,
    focusRing: HarvestCircleFocusRing,
) {
    val colors = HarvestCircleTheme.foundation.colors
    val width = HarvestCircleTheme.shell.dimensions.switchWidth
    val height = HarvestCircleTheme.shell.dimensions.switchHeight
    val knob = height - 4.dp
    val travel = width - knob - 4.dp
    val direction = LocalLayoutDirection.current
    val targetOffset =
        when {
            checked && direction == LayoutDirection.Ltr -> travel
            !checked && direction == LayoutDirection.Rtl -> travel
            else -> 0.dp
        }
    val duration = HarvestCircleTheme.component.motion.standardMillis
    val knobOffset =
        animateDpAsState(
            targetValue = targetOffset,
            animationSpec = if (duration == 0) snap() else tween(durationMillis = duration),
            label = "HarvestCircleSwitchKnob",
        ).value
    val track =
        when {
            !enabled && checked -> colors.action.primary.disabled
            !enabled -> colors.surface.sunken
            checked -> colors.action.primary.rest
            hovered -> colors.border.default
            else -> colors.surface.sunken
        }
    val border =
        when {
            !enabled -> colors.border.subtle
            checked -> lerp(colors.action.primary.rest, Color.Black, 0.16f)
            hovered -> colors.border.strong
            else -> colors.border.default
        }

    HarvestCircleMacFocusFrame(
        focused = focused,
        focusRing = focusRing,
        shape = CircleShape,
        ringShape = CircleShape,
    ) {
        Box(
            modifier =
                Modifier
                    .size(width = width, height = height)
                    .clip(CircleShape)
                    .background(track)
                    .border(BorderStroke(1.dp, border), CircleShape),
            contentAlignment = Alignment.CenterStart,
        ) {
            Box(
                modifier =
                    Modifier
                        .offset(x = 2.dp + knobOffset)
                        .size(knob)
                        .shadow(1.dp, CircleShape, clip = false)
                        .clip(CircleShape)
                        .background(
                            if (enabled) {
                                colors.surface.raised
                            } else {
                                colors.content.disabled
                            },
                        ).border(
                            BorderStroke(1.dp, colors.border.subtle),
                            CircleShape,
                        ),
            )
        }
    }
}

/** Canonical AppKit switch with an optional merged, clickable label. */
@Composable
public fun HarvestCircleSwitch(
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    label: String? = null,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
) {
    val sources = rememberHarvestCircleInteractionSources()
    val interactions = sources.harvestCircleInteractions(enabled)
    val toggleModifier =
        modifier
            .defaultMinSize(
                minWidth = HarvestCircleTheme.shell.dimensions.minimumInteractive,
                minHeight = HarvestCircleTheme.shell.dimensions.minimumInteractive,
            ).harvestCircleHoverable(sources = sources, enabled = enabled)
            .toggleable(
                value = checked,
                interactionSource = sources.activationSource,
                indication = null,
                enabled = enabled,
                role = Role.Switch,
                onValueChange = onCheckedChange,
            )

    if (label == null) {
        Box(
            modifier = toggleModifier,
            contentAlignment = Alignment.Center,
        ) {
            HarvestCircleSwitchVisual(
                checked = checked,
                enabled = enabled,
                hovered = interactions.hovered,
                focused = interactions.focused,
                focusRing = focusRing,
            )
        }
    } else {
        Row(
            modifier = toggleModifier.padding(vertical = HarvestCircleTheme.foundation.spacing.xs),
            horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            HarvestCircleText(
                text = label,
                modifier = Modifier.weight(1f),
                role = HarvestCircleTextRole.Body,
                tone = if (enabled) HarvestCircleContentTone.Primary else HarvestCircleContentTone.Disabled,
            )
            HarvestCircleSwitchVisual(
                checked = checked,
                enabled = enabled,
                hovered = interactions.hovered,
                focused = interactions.focused,
                focusRing = focusRing,
            )
        }
    }
}
