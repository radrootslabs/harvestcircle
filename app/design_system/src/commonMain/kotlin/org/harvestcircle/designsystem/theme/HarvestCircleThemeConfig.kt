package org.harvestcircle.designsystem.theme

import androidx.compose.runtime.Immutable

/** Controls whether HarvestCircle follows the system appearance or uses a fixed appearance. */
public enum class HarvestCircleThemeMode {
    System,
    Light,
    Dark,
}

/** Controls visual separation without changing the HarvestCircle brand. */
public enum class HarvestCircleContrast {
    Standard,
    High,
}

/** Controls visual density using pre-Liquid-Glass macOS metrics. */
public enum class HarvestCircleDensity {
    Compact,
    Comfortable,
}

/** Controls whether non-essential movement is animated. */
public enum class HarvestCircleMotionMode {
    Full,
    Reduced,
}

/**
 * Selects the interaction target policy independently from the visual control metrics.
 *
 * [Pointer] is the default for the desktop and browser application. Use [Touch] when the same
 * design-system module is hosted by an Android or other touch-first application.
 */
public enum class HarvestCircleInputMode {
    Pointer,
    Touch,
}

/** Product-supported text scales; arbitrary scaling is intentionally excluded. */
public enum class HarvestCircleTextScale(
    internal val factor: Float,
) {
    Standard(1F),
    Large(1.15F),
    ExtraLarge(1.3F),
}

/** The complete, orthogonal configuration for [HarvestCircleDesignTheme]. */
@Immutable
public data class HarvestCircleThemeConfig(
    public val mode: HarvestCircleThemeMode = HarvestCircleThemeMode.System,
    public val contrast: HarvestCircleContrast = HarvestCircleContrast.Standard,
    public val density: HarvestCircleDensity = HarvestCircleDensity.Comfortable,
    public val motion: HarvestCircleMotionMode = HarvestCircleMotionMode.Full,
    public val inputMode: HarvestCircleInputMode = HarvestCircleInputMode.Pointer,
    public val textScale: HarvestCircleTextScale = HarvestCircleTextScale.Standard,
)
