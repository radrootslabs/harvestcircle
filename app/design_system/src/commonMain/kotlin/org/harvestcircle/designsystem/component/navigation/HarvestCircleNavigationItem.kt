package org.harvestcircle.designsystem.component.navigation

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
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
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.component.HarvestCircleIconSize
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacFocusFrame
import org.harvestcircle.designsystem.internal.interaction.harvestCircleHoverable
import org.harvestcircle.designsystem.internal.interaction.harvestCircleInteractions
import org.harvestcircle.designsystem.internal.interaction.rememberHarvestCircleInteractionSources
import org.harvestcircle.designsystem.primitive.HarvestCircleIcon
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.jetbrains.compose.resources.DrawableResource

/**
 * Inset macOS sidebar row.
 *
 * [emphasized] represents an active key window. Set it to false for an inactive-window selection.
 * The optional focus ring is disabled by default.
 */
@Composable
public fun HarvestCircleNavigationItem(
    selected: Boolean,
    onClick: () -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    icon: DrawableResource? = null,
    enabled: Boolean = true,
    trailingContent: (@Composable RowScope.() -> Unit)? = null,
    emphasized: Boolean = true,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
) {
    val sources = rememberHarvestCircleInteractionSources()
    val interactions = sources.harvestCircleInteractions(enabled)
    val container =
        when {
            !enabled -> Color.Transparent
            selected && emphasized -> HarvestCircleTheme.foundation.colors.action.primary.rest
            selected -> HarvestCircleTheme.foundation.colors.surface.selected
            interactions.pressed -> HarvestCircleTheme.foundation.colors.action.ghost.pressed
            interactions.hovered -> HarvestCircleTheme.foundation.colors.action.ghost.hover
            else -> Color.Transparent
        }
    val contentColor =
        when {
            !enabled -> HarvestCircleTheme.foundation.colors.content.disabled
            selected && emphasized -> HarvestCircleTheme.foundation.colors.content.inverse
            else -> HarvestCircleTheme.foundation.colors.content.primary
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
            ProvideHarvestCircleContentColor(contentColor) {
                Row(
                    modifier =
                        Modifier
                            .height(HarvestCircleTheme.shell.dimensions.rowHeight)
                            .clip(HarvestCircleTheme.foundation.shapes.control)
                            .background(container)
                            .padding(horizontal = HarvestCircleTheme.foundation.spacing.md),
                    horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (icon != null) {
                        HarvestCircleIcon(
                            resource = icon,
                            contentDescription = null,
                            size = HarvestCircleIconSize.Medium,
                            tone = HarvestCircleContentTone.Inherit,
                        )
                    }

                    HarvestCircleText(
                        text = label,
                        modifier = Modifier.weight(1f),
                        role = HarvestCircleTextRole.Label,
                        tone = HarvestCircleContentTone.Inherit,
                        maxLines = 1,
                    )

                    trailingContent?.invoke(this)
                }
            }
        }
    }
}
