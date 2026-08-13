package org.harvestcircle.designsystem.shell

import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier

@Stable
public class HarvestCircleShellInteractionSources internal constructor(
    public val activation: MutableInteractionSource,
    public val hover: MutableInteractionSource,
)

@Immutable
public data class HarvestCircleShellInteractions(
    public val hovered: Boolean,
    public val pressed: Boolean,
    public val focused: Boolean,
)

@Composable
public fun rememberHarvestCircleShellInteractionSources(key: Any? = Unit): HarvestCircleShellInteractionSources =
    remember(key) { HarvestCircleShellInteractionSources(MutableInteractionSource(), MutableInteractionSource()) }

@Composable
public fun HarvestCircleShellInteractionSources.collectHarvestCircleShellInteractions(
    enabled: Boolean = true,
): HarvestCircleShellInteractions {
    val hovered by hover.collectIsHoveredAsState()
    val pressed by activation.collectIsPressedAsState()
    val focused by activation.collectIsFocusedAsState()
    return HarvestCircleShellInteractions(enabled && hovered, enabled && pressed, enabled && focused)
}

public fun Modifier.harvestCircleShellHoverable(
    sources: HarvestCircleShellInteractionSources,
    enabled: Boolean = true,
): Modifier = hoverable(sources.hover, enabled)
