package org.harvestcircle.designsystem.shell

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import org.harvestcircle.designsystem.primitive.HarvestCircleIcon
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.jetbrains.compose.resources.DrawableResource

@Composable
public fun HarvestCircleShellIconButton(
    onClick: () -> Unit,
    icon: DrawableResource,
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    selected: Boolean = false,
    showBorder: Boolean = false,
    controlSize: Dp = HarvestCircleShellMetrics.topBarSquareControlSize,
    iconSize: Dp = HarvestCircleShellMetrics.topBarIconSize,
) {
    val colors = HarvestCircleShellPalette
    val sources = rememberHarvestCircleShellInteractionSources(label)
    val interaction = sources.collectHarvestCircleShellInteractions(enabled)
    val container =
        when {
            !enabled -> Color.Transparent
            selected -> colors.navigationSelected
            interaction.pressed -> colors.navigationPressed
            interaction.hovered -> colors.navigationHover
            else -> Color.Transparent
        }
    val shape =
        androidx.compose.foundation.shape
            .RoundedCornerShape(HarvestCircleShellMetrics.controlRadius)
    Box(
        modifier =
            modifier
                .size(controlSize)
                .clip(shape)
                .background(container)
                .border(BorderStroke(1.dp, if (showBorder || selected) colors.border else Color.Transparent), shape)
                .harvestCircleShellHoverable(sources, enabled)
                .semantics {
                    contentDescription = label
                    role = Role.Button
                    if (!enabled) disabled()
                }.then(
                    if (enabled) {
                        Modifier.clickable(
                            interactionSource = sources.activation,
                            indication = null,
                            role = Role.Button,
                            onClick = onClick,
                        )
                    } else {
                        Modifier
                    },
                ),
        contentAlignment = Alignment.Center,
    ) {
        HarvestCircleIcon(
            resource = icon,
            contentDescription = null,
            modifier = Modifier.size(iconSize),
            tint = if (enabled) colors.contentSecondary else colors.contentDisabled,
        )
    }
}

@Composable
public fun HarvestCircleShellButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    primary: Boolean = false,
    destructive: Boolean = false,
    enabled: Boolean = true,
    leadingContent: (@Composable RowScope.() -> Unit)? = null,
) {
    val colors = HarvestCircleShellPalette
    val sources = rememberHarvestCircleShellInteractionSources(text)
    val interaction = sources.collectHarvestCircleShellInteractions(enabled)
    val filled = primary || destructive
    val active = if (destructive) Color(0xFF982018) else colors.accent
    val activeHover = if (destructive) Color(0xFFB42318) else colors.accentHover
    val activePressed = if (destructive) Color(0xFF7A1A14) else colors.accentPressed
    val background =
        when {
            !enabled -> colors.input
            filled && interaction.pressed -> activePressed
            filled && interaction.hovered -> activeHover
            filled -> active
            interaction.pressed -> colors.navigationPressed
            interaction.hovered -> colors.navigationHover
            else -> colors.raised
        }
    val foreground =
        if (!enabled) {
            colors.contentDisabled
        } else if (filled) {
            colors.onAccent
        } else {
            colors.contentPrimary
        }
    val shape =
        androidx.compose.foundation.shape
            .RoundedCornerShape(HarvestCircleShellMetrics.controlRadius)
    Row(
        modifier =
            modifier
                .height(32.dp)
                .clip(shape)
                .background(background)
                .border(BorderStroke(1.dp, if (filled) background else colors.border), shape)
                .harvestCircleShellHoverable(sources, enabled)
                .semantics {
                    role = Role.Button
                    contentDescription = text
                    if (!enabled) disabled()
                }.then(
                    if (enabled) {
                        Modifier.clickable(
                            interactionSource = sources.activation,
                            indication = null,
                            role = Role.Button,
                            onClick = onClick,
                        )
                    } else {
                        Modifier
                    },
                ).padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        leadingContent?.invoke(this)
        HarvestCircleShellText(text, role = HarvestCircleShellTextRole.Label, color = foreground, maxLines = 1)
    }
}

@Composable
public fun HarvestCircleShellNavigationItem(
    label: String,
    icon: DrawableResource,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    compact: Boolean = false,
) {
    val colors = HarvestCircleShellPalette
    val sources = rememberHarvestCircleShellInteractionSources("navigation:$label")
    val interaction = sources.collectHarvestCircleShellInteractions(enabled)
    val background =
        when {
            selected -> colors.navigationSelected
            interaction.pressed -> colors.navigationPressed
            interaction.hovered -> colors.navigationHover
            else -> Color.Transparent
        }
    val content = if (enabled) colors.contentSecondary else colors.contentDisabled
    val shape =
        androidx.compose.foundation.shape
            .RoundedCornerShape(HarvestCircleShellMetrics.navigationRadius)
    Row(
        modifier =
            modifier
                .fillMaxWidth()
                .height(HarvestCircleShellMetrics.sidebarNavigationRowHeight)
                .clip(shape)
                .background(background)
                .harvestCircleShellHoverable(sources, enabled)
                .semantics { if (!enabled) disabled() }
                .then(
                    if (enabled) Modifier.selectable(selected, sources.activation, null, true, Role.Tab, onClick) else Modifier,
                ).padding(horizontal = HarvestCircleShellMetrics.sidebarNavigationHorizontalPadding),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = if (compact) Arrangement.Center else Arrangement.Start,
    ) {
        HarvestCircleIcon(
            icon,
            if (compact) label else null,
            Modifier.size(HarvestCircleShellMetrics.sidebarNavigationIconSize),
            tint = content,
        )
        if (!compact) {
            Spacer(Modifier.width(HarvestCircleShellMetrics.sidebarNavigationIconGap))
            HarvestCircleShellText(
                label,
                Modifier.weight(1f),
                if (selected) HarvestCircleShellTextRole.BodyStrong else HarvestCircleShellTextRole.Body,
                if (selected) colors.contentPrimary else content,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
public fun HarvestCircleShellTab(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: DrawableResource? = null,
) {
    val colors = HarvestCircleShellPalette
    val sources = rememberHarvestCircleShellInteractionSources("tab:$label")
    val interaction = sources.collectHarvestCircleShellInteractions()
    val shape =
        androidx.compose.foundation.shape
            .RoundedCornerShape(HarvestCircleShellMetrics.navigationRadius)
    val background =
        when {
            selected -> colors.navigationSelected
            interaction.pressed -> colors.navigationPressed
            interaction.hovered -> colors.navigationHover
            else -> Color.Transparent
        }
    Row(
        modifier =
            modifier
                .defaultMinSize(minWidth = 88.dp)
                .height(32.dp)
                .clip(shape)
                .background(background)
                .border(BorderStroke(1.dp, if (selected) colors.border else Color.Transparent), shape)
                .harvestCircleShellHoverable(sources)
                .selectable(selected, sources.activation, null, true, Role.Tab, onClick)
                .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        icon?.let { HarvestCircleIcon(it, null, Modifier.size(16.dp), tint = colors.contentSecondary) }
        HarvestCircleShellText(
            label,
            role = if (selected) HarvestCircleShellTextRole.BodyStrong else HarvestCircleShellTextRole.Body,
            maxLines = 1,
        )
    }
}

@Composable
public fun HarvestCircleShellSearchField(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    modifier: Modifier = Modifier,
    compact: Boolean = false,
    visualTransformation: VisualTransformation = VisualTransformation.None,
) {
    val colors = HarvestCircleShellPalette
    val sources = rememberHarvestCircleShellInteractionSources("shell-search")
    val interaction = sources.collectHarvestCircleShellInteractions()
    val shape =
        androidx.compose.foundation.shape
            .RoundedCornerShape(HarvestCircleShellMetrics.controlRadius)
    Row(
        modifier =
            modifier
                .width(if (compact) HarvestCircleShellMetrics.topBarSearchCompactWidth else HarvestCircleShellMetrics.topBarSearchWidth)
                .height(32.dp)
                .clip(shape)
                .background(colors.input)
                .border(BorderStroke(1.dp, if (interaction.focused) colors.accent else colors.border), shape)
                .padding(horizontal = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        HarvestCircleIcon(HarvestCircleIcons.Search, null, Modifier.size(15.dp), tint = colors.contentMuted)
        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            modifier = Modifier.weight(1f),
            textStyle =
                HarvestCircleTheme.foundation.typography.body
                    .copy(color = colors.contentPrimary),
            interactionSource = sources.activation,
            cursorBrush = SolidColor(colors.accent),
            visualTransformation = visualTransformation,
            singleLine = true,
            decorationBox = { inner ->
                Box(contentAlignment = Alignment.CenterStart) {
                    if (value.isEmpty()) HarvestCircleShellText(placeholder, color = colors.contentMuted, maxLines = 1)
                    inner()
                }
            },
        )
    }
}
