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
import androidx.compose.foundation.selection.selectable
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
import androidx.compose.ui.semantics.SemanticsPropertyKey
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
import org.harvestcircle.design.ColorToken
import org.harvestcircle.design.FontWeightToken
import org.harvestcircle.design.HarvestCircleDesign
import org.harvestcircle.design.HarvestCirclePalette
import org.harvestcircle.design.TypographyToken

enum class ShellTextRole { ScreenTitle, SectionTitle, CardTitle, Body, Secondary, Protocol, Button }

enum class ShellButtonKind { Primary, Secondary, Quiet, Destructive }

sealed interface ShellControlBackground {
    data class Solid(
        val color: ColorToken,
    ) : ShellControlBackground

    data object Transparent : ShellControlBackground
}

data class ShellControlVisuals(
    val background: ShellControlBackground,
    val foreground: ColorToken,
    val border: ColorToken,
    val focusRing: ColorToken?,
)

internal val ShellControlBackgroundKey = SemanticsPropertyKey<String>("ShellControlBackground")
internal val ShellControlForegroundKey = SemanticsPropertyKey<String>("ShellControlForeground")
internal val ShellControlBorderKey = SemanticsPropertyKey<String>("ShellControlBorder")

fun resolveShellControlVisuals(
    kind: ShellButtonKind,
    enabled: Boolean,
    selected: Boolean,
    focused: Boolean,
    pressed: Boolean,
    hovered: Boolean,
    palette: HarvestCirclePalette,
): ShellControlVisuals {
    if (!enabled) {
        return ShellControlVisuals(
            background = ShellControlBackground.Solid(palette.surfaceSecondary),
            foreground = palette.textSecondary,
            border = palette.border,
            focusRing = null,
        )
    }
    val background =
        when {
            selected -> ShellControlBackground.Solid(if (hovered && !pressed) palette.primaryHover else palette.primary)
            kind == ShellButtonKind.Primary ->
                ShellControlBackground.Solid(if (hovered && !pressed) palette.primaryHover else palette.primary)
            kind == ShellButtonKind.Destructive -> ShellControlBackground.Solid(palette.critical)
            kind == ShellButtonKind.Secondary -> ShellControlBackground.Solid(palette.surfaceSecondary)
            pressed || hovered -> ShellControlBackground.Solid(palette.surfaceSecondary)
            else -> ShellControlBackground.Transparent
        }
    val foreground =
        if (selected || kind == ShellButtonKind.Primary || kind == ShellButtonKind.Destructive) {
            palette.surface
        } else {
            palette.textPrimary
        }
    return ShellControlVisuals(
        background = background,
        foreground = foreground,
        border = if (focused) palette.focus else palette.border,
        focusRing = if (focused) palette.focus else null,
    )
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
    kind: ShellButtonKind = ShellButtonKind.Secondary,
) {
    ShellControl(
        label = label,
        description = description,
        onClick = onClick,
        modifier = modifier,
        enabled = enabled,
        selected = null,
        kind = kind,
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
    ShellControl(
        label = label,
        description = description,
        onClick = { if (!selected) onClick() },
        modifier = modifier,
        enabled = enabled,
        selected = selected,
        kind = ShellButtonKind.Quiet,
    )
}

@Composable
private fun ShellControl(
    label: String,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier,
    enabled: Boolean,
    selected: Boolean?,
    kind: ShellButtonKind,
) {
    val interactionSource = remember { MutableInteractionSource() }
    val hovered by interactionSource.collectIsHoveredAsState()
    val pressed by interactionSource.collectIsPressedAsState()
    var focused by remember { mutableStateOf(false) }
    val palette = LocalHarvestCirclePalette.current
    val visuals = resolveShellControlVisuals(kind, enabled, selected == true, focused, pressed, hovered, palette)
    val actionModifier =
        if (selected == null) {
            Modifier.clickable(interactionSource, indication = null, enabled = enabled, role = Role.Button, onClick = onClick)
        } else {
            Modifier.selectable(
                selected = selected,
                interactionSource = interactionSource,
                indication = null,
                enabled = enabled,
                role = Role.Tab,
                onClick = onClick,
            )
        }
    Box(
        modifier
            .heightIn(min = HarvestCircleDesign.MINIMUM_TARGET_DP.dp)
            .background(visuals.background.toComposeColor(), RoundedCornerShape(LocalHarvestCircleShapes.current.controlRadiusDp.dp))
            .border(
                HarvestCircleDesign.BORDER_DP.dp,
                visuals.border.toComposeColor(),
                RoundedCornerShape(LocalHarvestCircleShapes.current.controlRadiusDp.dp),
            ).onFocusChanged { focused = it.isFocused }
            .hoverable(interactionSource, enabled)
            .then(actionModifier)
            .focusable(enabled, interactionSource)
            .semantics {
                contentDescription = description
                role = if (selected == null) Role.Button else Role.Tab
                selected?.let { this.selected = it }
                this[ShellControlBackgroundKey] = visuals.background.semanticValue()
                this[ShellControlForegroundKey] = visuals.foreground.hex
                this[ShellControlBorderKey] = visuals.border.hex
                if (!enabled) disabled()
            }.padding(horizontal = HarvestCircleDesign.spacingDp[3].dp, vertical = HarvestCircleDesign.spacingDp[2].dp),
    ) {
        ShellText(label, textRole = ShellTextRole.Button, color = visuals.foreground.toComposeColor())
    }
}

private fun ShellControlBackground.toComposeColor(): Color =
    when (this) {
        is ShellControlBackground.Solid -> color.toComposeColor()
        ShellControlBackground.Transparent -> Color.Transparent
    }

private fun ShellControlBackground.semanticValue(): String =
    when (this) {
        is ShellControlBackground.Solid -> color.hex
        ShellControlBackground.Transparent -> "transparent"
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
