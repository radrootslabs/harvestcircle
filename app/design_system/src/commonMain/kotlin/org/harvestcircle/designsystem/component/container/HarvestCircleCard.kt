package org.harvestcircle.designsystem.component.container

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacFocusFrame
import org.harvestcircle.designsystem.internal.interaction.harvestCircleHoverable
import org.harvestcircle.designsystem.internal.interaction.harvestCircleInteractions
import org.harvestcircle.designsystem.internal.interaction.rememberHarvestCircleInteractionSources
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

public enum class HarvestCircleCardVariant {
    Default,
    Raised,
    Outlined,
    Selected,
}

public enum class HarvestCircleCardPadding {
    None,
    Compact,
    Default,
}

private data class CardVisuals(
    val color: Color,
    val border: BorderStroke?,
    val elevation: Dp,
)

@Composable
private fun cardVisuals(variant: HarvestCircleCardVariant): CardVisuals =
    when (variant) {
        HarvestCircleCardVariant.Default ->
            CardVisuals(
                color = HarvestCircleTheme.foundation.colors.surface.base,
                border = BorderStroke(1.dp, HarvestCircleTheme.foundation.colors.border.subtle),
                elevation = HarvestCircleTheme.component.elevations.flat,
            )

        HarvestCircleCardVariant.Raised ->
            CardVisuals(
                color = HarvestCircleTheme.foundation.colors.surface.raised,
                border = BorderStroke(1.dp, HarvestCircleTheme.foundation.colors.border.subtle),
                elevation = HarvestCircleTheme.component.elevations.raised,
            )

        HarvestCircleCardVariant.Outlined ->
            CardVisuals(
                color = HarvestCircleTheme.foundation.colors.surface.base,
                border = BorderStroke(1.dp, HarvestCircleTheme.foundation.colors.border.default),
                elevation = HarvestCircleTheme.component.elevations.flat,
            )

        HarvestCircleCardVariant.Selected ->
            CardVisuals(
                color = HarvestCircleTheme.foundation.colors.surface.selected,
                border = BorderStroke(1.dp, HarvestCircleTheme.foundation.colors.border.selected),
                elevation = HarvestCircleTheme.component.elevations.flat,
            )
    }

@Composable
private fun cardPadding(padding: HarvestCircleCardPadding): PaddingValues =
    when (padding) {
        HarvestCircleCardPadding.None -> PaddingValues(0.dp)
        HarvestCircleCardPadding.Compact -> PaddingValues(HarvestCircleTheme.foundation.spacing.md)
        HarvestCircleCardPadding.Default -> PaddingValues(HarvestCircleTheme.foundation.spacing.lg)
    }

@Composable
private fun HarvestCircleCardFrame(
    color: Color,
    border: BorderStroke?,
    elevation: Dp,
    modifier: Modifier = Modifier,
    padding: HarvestCircleCardPadding,
    content: @Composable BoxScope.() -> Unit,
) {
    ProvideHarvestCircleContentColor(HarvestCircleTheme.foundation.colors.content.primary) {
        Box(
            modifier =
                modifier
                    .then(
                        if (elevation > 0.dp) {
                            Modifier.shadow(elevation, HarvestCircleTheme.foundation.shapes.card, clip = false)
                        } else {
                            Modifier
                        },
                    ).clip(HarvestCircleTheme.foundation.shapes.card)
                    .background(color)
                    .then(
                        if (border != null) {
                            Modifier.border(border, HarvestCircleTheme.foundation.shapes.card)
                        } else {
                            Modifier
                        },
                    ).padding(cardPadding(padding)),
            content = content,
        )
    }
}

/** Flat AppKit group/card container. */
@Composable
public fun HarvestCircleCard(
    modifier: Modifier = Modifier,
    variant: HarvestCircleCardVariant = HarvestCircleCardVariant.Default,
    padding: HarvestCircleCardPadding = HarvestCircleCardPadding.Default,
    content: @Composable BoxScope.() -> Unit,
) {
    val visuals = cardVisuals(variant)
    HarvestCircleCardFrame(
        color = visuals.color,
        border = visuals.border,
        elevation = visuals.elevation,
        modifier = modifier,
        padding = padding,
        content = content,
    )
}

/** Clickable card with Foundation interaction semantics and no Material surface behavior. */
@Composable
public fun HarvestCircleClickableCard(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    variant: HarvestCircleCardVariant = HarvestCircleCardVariant.Default,
    padding: HarvestCircleCardPadding = HarvestCircleCardPadding.Default,
    enabled: Boolean = true,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
    content: @Composable BoxScope.() -> Unit,
) {
    val sources = rememberHarvestCircleInteractionSources()
    val interactions = sources.harvestCircleInteractions(enabled)
    val visuals = cardVisuals(variant)
    val color =
        when {
            !enabled -> visuals.color
            interactions.pressed ->
                lerp(visuals.color, HarvestCircleTheme.foundation.colors.action.ghost.pressed, 0.55f)
            interactions.hovered ->
                lerp(visuals.color, HarvestCircleTheme.foundation.colors.action.ghost.hover, 0.40f)
            else -> visuals.color
        }

    Box(
        modifier =
            modifier
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
            modifier = Modifier.fillMaxWidth(),
            shape = HarvestCircleTheme.foundation.shapes.card,
            ringShape = HarvestCircleTheme.foundation.shapes.cardFocusRing,
        ) {
            HarvestCircleCardFrame(
                color = color,
                border = visuals.border,
                elevation = visuals.elevation,
                modifier = Modifier.fillMaxWidth(),
                padding = padding,
                content = content,
            )
        }
    }
}

/** Canonical labeled group box for settings and inspector forms. */
@Composable
public fun HarvestCircleGroupBox(
    modifier: Modifier = Modifier,
    title: String? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(modifier = modifier) {
        if (title != null) {
            HarvestCircleText(
                text = title,
                modifier =
                    Modifier.padding(
                        start = HarvestCircleTheme.foundation.spacing.sm,
                        bottom = HarvestCircleTheme.foundation.spacing.xs,
                    ),
                role = HarvestCircleTextRole.LabelSmall,
                tone = HarvestCircleContentTone.Secondary,
            )
        }

        HarvestCircleCard(
            variant = HarvestCircleCardVariant.Outlined,
            padding = HarvestCircleCardPadding.Default,
        ) {
            Column(content = content)
        }
    }
}
