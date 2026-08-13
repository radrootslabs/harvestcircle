package org.harvestcircle.designsystem.component.navigation

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacBezel
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacFocusFrame
import org.harvestcircle.designsystem.internal.chrome.harvestCircleMacControlBrush
import org.harvestcircle.designsystem.internal.interaction.harvestCircleHoverable
import org.harvestcircle.designsystem.internal.interaction.harvestCircleInteractions
import org.harvestcircle.designsystem.internal.interaction.rememberHarvestCircleInteractionSources
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** AppKit-style segmented control container. */
@Composable
public fun HarvestCircleTabRow(
    modifier: Modifier = Modifier,
    content: @Composable RowScope.() -> Unit,
) {
    Row(
        modifier =
            modifier
                .clip(HarvestCircleTheme.foundation.shapes.control)
                .background(HarvestCircleTheme.foundation.colors.surface.sunken)
                .border(
                    width = 1.dp,
                    color = HarvestCircleTheme.foundation.colors.border.default,
                    shape = HarvestCircleTheme.foundation.shapes.control,
                ).padding(HarvestCircleTheme.foundation.spacing.xxs),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

/** One selectable segment inside [HarvestCircleTabRow]. */
@Composable
public fun RowScope.HarvestCircleTab(
    selected: Boolean,
    onClick: () -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
) {
    val sources = rememberHarvestCircleInteractionSources()
    val interactions = sources.harvestCircleInteractions(enabled)
    val container =
        when {
            !enabled -> Color.Transparent
            selected -> HarvestCircleTheme.foundation.colors.surface.raised
            interactions.pressed -> HarvestCircleTheme.foundation.colors.action.ghost.pressed
            interactions.hovered -> HarvestCircleTheme.foundation.colors.action.ghost.hover
            else -> Color.Transparent
        }
    val border =
        if (selected) {
            HarvestCircleTheme.foundation.colors.border.subtle
        } else {
            Color.Transparent
        }

    Box(
        modifier =
            modifier
                .defaultMinSize(minHeight = HarvestCircleTheme.shell.dimensions.minimumInteractive)
                .harvestCircleHoverable(sources = sources, enabled = enabled)
                .selectable(
                    selected = selected,
                    interactionSource = sources.activationSource,
                    indication = null,
                    enabled = enabled,
                    role = Role.Tab,
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
                modifier = Modifier.height(HarvestCircleTheme.shell.dimensions.controlMedium),
                brush = harvestCircleMacControlBrush(container),
                border = BorderStroke(1.dp, border),
                shape = HarvestCircleTheme.foundation.shapes.control,
                shadowElevation = if (selected) HarvestCircleTheme.component.elevations.raised else 0.dp,
            ) {
                ProvideHarvestCircleContentColor(
                    if (enabled) {
                        HarvestCircleTheme.foundation.colors.content.primary
                    } else {
                        HarvestCircleTheme.foundation.colors.content.disabled
                    },
                ) {
                    HarvestCircleText(
                        text = label,
                        modifier =
                            Modifier.padding(
                                horizontal = HarvestCircleTheme.foundation.spacing.lg,
                                vertical = HarvestCircleTheme.foundation.spacing.xs,
                            ),
                        role = HarvestCircleTextRole.Label,
                        tone =
                            if (enabled) {
                                HarvestCircleContentTone.Inherit
                            } else {
                                HarvestCircleContentTone.Disabled
                            },
                        maxLines = 1,
                    )
                }
            }
        }
    }
}
