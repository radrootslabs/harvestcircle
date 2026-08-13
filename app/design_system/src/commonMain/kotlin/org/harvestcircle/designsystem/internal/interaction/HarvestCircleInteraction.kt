package org.harvestcircle.designsystem.internal.interaction

import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier

/**
 * Keeps activation/focus interactions separate from pointer hover interactions.
 *
 * `clickable`, `selectable`, and `toggleable` own press and focus semantics. A dedicated
 * [hoverSource] is attached with [Modifier.hoverable] so pointer enter remains active for the full
 * time the pointer is inside the component. This prevents transient enter/exit flashes caused by
 * relying on activation modifiers as the visual hover source on desktop and web targets.
 */
@Stable
internal class HarvestCircleInteractionSources internal constructor(
    internal val activationSource: MutableInteractionSource,
    internal val hoverSource: MutableInteractionSource,
)

@Composable
internal fun rememberHarvestCircleInteractionSources(vararg keys: Any?): HarvestCircleInteractionSources =
    remember(*keys) {
        HarvestCircleInteractionSources(
            activationSource = MutableInteractionSource(),
            hoverSource = MutableInteractionSource(),
        )
    }

@Immutable
internal data class HarvestCircleInteractionSnapshot(
    val pressed: Boolean,
    val hovered: Boolean,
    val focused: Boolean,
)

@Composable
internal fun HarvestCircleInteractionSources.harvestCircleInteractions(enabled: Boolean): HarvestCircleInteractionSnapshot =
    if (!enabled) {
        HarvestCircleInteractionSnapshot(
            pressed = false,
            hovered = false,
            focused = false,
        )
    } else {
        HarvestCircleInteractionSnapshot(
            pressed = activationSource.collectIsPressedAsState().value,
            hovered = hoverSource.collectIsHoveredAsState().value,
            focused = activationSource.collectIsFocusedAsState().value,
        )
    }

internal fun Modifier.harvestCircleHoverable(
    sources: HarvestCircleInteractionSources,
    enabled: Boolean,
): Modifier =
    hoverable(
        interactionSource = sources.hoverSource,
        enabled = enabled,
    )
