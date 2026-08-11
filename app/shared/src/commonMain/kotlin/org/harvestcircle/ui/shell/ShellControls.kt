package org.harvestcircle.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.harvestcircle.design.FontWeightToken
import org.harvestcircle.design.HarvestCircleDesign
import org.harvestcircle.design.TypographyToken

enum class ShellTextRole { ScreenTitle, SectionTitle, CardTitle, Body, Secondary, Protocol, Button }

enum class ShellButtonKind { Primary, Secondary, Quiet, Destructive }

enum class ShellControlVisualState { Normal, Hovered, Pressed, Focused, Selected, Disabled }

fun shellControlVisualState(
    enabled: Boolean,
    selected: Boolean,
    focused: Boolean,
    pressed: Boolean,
    hovered: Boolean,
): ShellControlVisualState =
    when {
        !enabled && selected -> ShellControlVisualState.Selected
        !enabled -> ShellControlVisualState.Disabled
        focused -> ShellControlVisualState.Focused
        pressed -> ShellControlVisualState.Pressed
        selected -> ShellControlVisualState.Selected
        hovered -> ShellControlVisualState.Hovered
        else -> ShellControlVisualState.Normal
    }

@Composable
fun ShellSurface(
    modifier: Modifier = Modifier,
    secondary: Boolean = false,
    content: @Composable () -> Unit,
) {
    val palette = LocalHarvestCirclePalette.current
    Box(
        modifier.background(
            if (secondary) palette.surfaceSecondary.toComposeColor() else palette.surface.toComposeColor(),
            RoundedCornerShape(LocalHarvestCircleShapes.current.surfaceRadiusDp.dp),
        ),
    ) {
        content()
    }
}

@Composable
fun ShellText(
    text: String,
    modifier: Modifier = Modifier,
    textRole: ShellTextRole = ShellTextRole.Body,
    color: Color? = null,
) {
    val typography = LocalHarvestCircleTypography.current
    val token =
        when (textRole) {
            ShellTextRole.ScreenTitle -> typography.screenTitle
            ShellTextRole.SectionTitle -> typography.sectionTitle
            ShellTextRole.CardTitle -> typography.cardTitle
            ShellTextRole.Body -> typography.body
            ShellTextRole.Secondary -> typography.secondary
            ShellTextRole.Protocol -> typography.protocol
            ShellTextRole.Button -> typography.button
        }
    val palette = LocalHarvestCirclePalette.current
    BasicText(
        text,
        modifier,
        style =
            token.toTextStyle(
                color
                    ?: if (textRole ==
                        ShellTextRole.Secondary
                    ) {
                        palette.textSecondary.toComposeColor()
                    } else {
                        palette.textPrimary.toComposeColor()
                    },
            ),
    )
}

@Composable
fun ShellButton(
    label: String,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    selected: Boolean = false,
    kind: ShellButtonKind = ShellButtonKind.Secondary,
) {
    val interactionSource = remember { MutableInteractionSource() }
    val hovered by interactionSource.collectIsHoveredAsState()
    val pressed by interactionSource.collectIsPressedAsState()
    var focused by remember { mutableStateOf(false) }
    val state = shellControlVisualState(enabled, selected, focused, pressed, hovered)
    val palette = LocalHarvestCirclePalette.current
    val background =
        when (state) {
            ShellControlVisualState.Disabled -> palette.surfaceSecondary.toComposeColor()
            ShellControlVisualState.Hovered -> palette.primaryHover.toComposeColor()
            ShellControlVisualState.Pressed -> palette.primary.toComposeColor()
            ShellControlVisualState.Focused -> palette.surface.toComposeColor()
            ShellControlVisualState.Selected -> palette.primary.toComposeColor()
            ShellControlVisualState.Normal ->
                when (kind) {
                    ShellButtonKind.Primary -> palette.primary.toComposeColor()
                    ShellButtonKind.Destructive -> palette.critical.toComposeColor()
                    ShellButtonKind.Secondary -> palette.surfaceSecondary.toComposeColor()
                    ShellButtonKind.Quiet -> Color.Transparent
                }
        }
    val foreground =
        if (state == ShellControlVisualState.Disabled) {
            palette.textSecondary.toComposeColor()
        } else if (state in setOf(ShellControlVisualState.Hovered, ShellControlVisualState.Pressed, ShellControlVisualState.Selected) ||
            kind == ShellButtonKind.Primary ||
            kind == ShellButtonKind.Destructive
        ) {
            palette.surface.toComposeColor()
        } else {
            palette.textPrimary.toComposeColor()
        }
    Box(
        modifier
            .heightIn(min = HarvestCircleDesign.MINIMUM_TARGET_DP.dp)
            .background(background, RoundedCornerShape(LocalHarvestCircleShapes.current.controlRadiusDp.dp))
            .border(
                HarvestCircleDesign.BORDER_DP.dp,
                if (focused) palette.focus.toComposeColor() else palette.border.toComposeColor(),
                RoundedCornerShape(LocalHarvestCircleShapes.current.controlRadiusDp.dp),
            ).onFocusChanged { focused = it.isFocused }
            .hoverable(interactionSource, enabled)
            .clickable(interactionSource, indication = null, enabled = enabled, role = Role.Button, onClick = onClick)
            .focusable(enabled, interactionSource)
            .semantics {
                contentDescription = description
                role = Role.Button
                this.selected = selected
                if (!enabled) disabled()
            }.padding(horizontal = HarvestCircleDesign.spacingDp[3].dp, vertical = HarvestCircleDesign.spacingDp[2].dp),
    ) {
        ShellText(label, textRole = ShellTextRole.Button, color = foreground)
    }
}

@Composable
fun ShellIconButton(
    glyph: String,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    ShellButton(
        glyph,
        description,
        onClick,
        modifier.sizeIn(minWidth = HarvestCircleDesign.MINIMUM_TARGET_DP.dp),
        enabled,
        kind = ShellButtonKind.Quiet,
    )
}

@Composable
fun ShellTab(
    label: String,
    description: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    ShellButton(label, description, onClick, modifier, enabled, selected, ShellButtonKind.Quiet)
}

@Composable
fun ShellTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    visualTransformation: VisualTransformation = VisualTransformation.None,
) {
    var focused by remember { mutableStateOf(false) }
    val palette = LocalHarvestCirclePalette.current
    Column {
        ShellText(label, textRole = ShellTextRole.Secondary)
        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            enabled = enabled,
            visualTransformation = visualTransformation,
            textStyle = LocalHarvestCircleTypography.current.body.toTextStyle(palette.textPrimary.toComposeColor()),
            modifier =
                modifier
                    .fillMaxWidth()
                    .heightIn(min = HarvestCircleDesign.PRIMARY_CONTROL_DP.dp)
                    .background(palette.surface.toComposeColor(), RoundedCornerShape(LocalHarvestCircleShapes.current.controlRadiusDp.dp))
                    .border(
                        HarvestCircleDesign.BORDER_DP.dp,
                        if (focused) palette.focus.toComposeColor() else palette.border.toComposeColor(),
                        RoundedCornerShape(LocalHarvestCircleShapes.current.controlRadiusDp.dp),
                    ).onFocusChanged { focused = it.isFocused }
                    .semantics {
                        contentDescription = label
                        if (!enabled) disabled()
                    }.padding(PaddingValues(HarvestCircleDesign.spacingDp[3].dp)),
            decorationBox = { innerTextField ->
                Box {
                    if (value.isEmpty()) ShellText(placeholder, textRole = ShellTextRole.Secondary)
                    innerTextField()
                }
            },
        )
    }
}

@Composable
fun ShellBadge(
    label: String,
    modifier: Modifier = Modifier,
) {
    val palette = LocalHarvestCirclePalette.current
    Box(
        modifier
            .semantics(mergeDescendants = true) {}
            .background(palette.surfaceSecondary.toComposeColor(), RoundedCornerShape(LocalHarvestCircleShapes.current.smallRadiusDp.dp))
            .border(
                HarvestCircleDesign.BORDER_DP.dp,
                palette.border.toComposeColor(),
                RoundedCornerShape(LocalHarvestCircleShapes.current.smallRadiusDp.dp),
            ).padding(horizontal = HarvestCircleDesign.spacingDp[2].dp, vertical = HarvestCircleDesign.spacingDp[1].dp),
    ) {
        ShellText(label, textRole = ShellTextRole.Secondary)
    }
}

@Composable
fun ShellCard(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val palette = LocalHarvestCirclePalette.current
    Box(
        modifier
            .background(palette.surface.toComposeColor(), RoundedCornerShape(LocalHarvestCircleShapes.current.surfaceRadiusDp.dp))
            .border(
                HarvestCircleDesign.BORDER_DP.dp,
                palette.border.toComposeColor(),
                RoundedCornerShape(LocalHarvestCircleShapes.current.surfaceRadiusDp.dp),
            ).padding(HarvestCircleDesign.spacingDp[4].dp),
    ) {
        content()
    }
}

@Composable
fun ShellDivider(modifier: Modifier = Modifier) {
    Box(
        modifier
            .fillMaxWidth()
            .heightIn(
                min = HarvestCircleDesign.BORDER_DP.dp,
            ).background(LocalHarvestCirclePalette.current.border.toComposeColor()),
    )
}

private fun TypographyToken.toTextStyle(color: Color): TextStyle =
    TextStyle(
        color = color,
        fontSize = sizeSp.sp,
        fontWeight = if (weight == FontWeightToken.Semibold) FontWeight.SemiBold else FontWeight.Normal,
        fontFamily = if (monospace) FontFamily.Monospace else FontFamily.Default,
    )
