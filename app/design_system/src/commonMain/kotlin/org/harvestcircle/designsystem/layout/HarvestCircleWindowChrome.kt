package org.harvestcircle.designsystem.layout

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.isSpecified

/** The physical window edge occupied by host-owned chrome. */
public enum class HarvestCircleWindowChromeEdge {
    Left,
    Right,
}

/**
 * A host-owned rectangle at the top of a window that application foreground content must avoid.
 *
 * [width] and [height] include the host's required safety gap. Backgrounds may remain full-bleed.
 */
@Immutable
public data class HarvestCircleWindowChromeExclusion(
    public val edge: HarvestCircleWindowChromeEdge,
    public val width: Dp,
    public val height: Dp,
) {
    init {
        requireValidWindowChromeDimension("width", width)
        requireValidWindowChromeDimension("height", height)
    }

    public companion object {
        public val None: HarvestCircleWindowChromeExclusion =
            HarvestCircleWindowChromeExclusion(
                edge = HarvestCircleWindowChromeEdge.Left,
                width = 0.dp,
                height = 0.dp,
            )
    }
}

/** Resolved physical clearances for a top-edge region inside a window. */
@Immutable
public data class HarvestCircleWindowChromeClearance(
    public val topBandHeight: Dp,
    public val left: Dp,
    public val right: Dp,
)

private val LocalHarvestCircleWindowChromeExclusion =
    staticCompositionLocalOf<HarvestCircleWindowChromeExclusion> {
        error("HarvestCircleWindowChromeEnvironment is missing from the composition")
    }

/** Read-only access to the host-owned exclusion for the current window. */
public object HarvestCircleWindowChrome {
    public val exclusion: HarvestCircleWindowChromeExclusion
        @Composable
        @ReadOnlyComposable
        get() = LocalHarvestCircleWindowChromeExclusion.current
}

/** Installs immutable host chrome geometry for exactly one application window. */
@Composable
public fun HarvestCircleWindowChromeEnvironment(
    exclusion: HarvestCircleWindowChromeExclusion,
    content: @Composable () -> Unit,
) {
    CompositionLocalProvider(
        LocalHarvestCircleWindowChromeExclusion provides exclusion,
        content = content,
    )
}

/**
 * Resolves the portion of [exclusion] intersecting a top-edge region.
 *
 * All horizontal inputs use window coordinates. The result is clamped to the region, so extremely
 * narrow windows fail safely without producing negative or impossible padding.
 */
public fun resolveHarvestCircleWindowChromeClearance(
    exclusion: HarvestCircleWindowChromeExclusion,
    windowWidth: Dp,
    regionLeft: Dp,
    regionWidth: Dp,
    minimumTopBandHeight: Dp,
): HarvestCircleWindowChromeClearance {
    requireValidWindowChromeDimension("windowWidth", windowWidth)
    requireValidWindowChromeDimension("regionLeft", regionLeft)
    requireValidWindowChromeDimension("regionWidth", regionWidth)
    requireValidWindowChromeDimension("minimumTopBandHeight", minimumTopBandHeight)
    require(regionLeft + regionWidth <= windowWidth) {
        "Window chrome region must remain inside the window"
    }

    val exclusionWidth = minOf(exclusion.width, windowWidth)
    val regionRight = regionLeft + regionWidth
    val left =
        if (exclusion.edge == HarvestCircleWindowChromeEdge.Left) {
            (exclusionWidth - regionLeft).coerceIn(0.dp, regionWidth)
        } else {
            0.dp
        }
    val right =
        if (exclusion.edge == HarvestCircleWindowChromeEdge.Right) {
            (regionRight - (windowWidth - exclusionWidth)).coerceIn(0.dp, regionWidth)
        } else {
            0.dp
        }

    return HarvestCircleWindowChromeClearance(
        topBandHeight = maxOf(minimumTopBandHeight, exclusion.height),
        left = left,
        right = right,
    )
}

private fun requireValidWindowChromeDimension(
    name: String,
    value: Dp,
) {
    require(value.isSpecified && value.value.isFinite() && value >= 0.dp) {
        "Window chrome $name must be a finite, non-negative Dp value"
    }
}
