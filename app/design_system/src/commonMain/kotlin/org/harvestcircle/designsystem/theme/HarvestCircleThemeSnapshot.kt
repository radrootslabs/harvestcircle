package org.harvestcircle.designsystem.theme

import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.runtime.Immutable
import org.harvestcircle.designsystem.theme.color.HarvestCircleColorSchemes
import org.harvestcircle.designsystem.theme.color.HarvestCircleColors

@Immutable
public class HarvestCircleFoundationTokens internal constructor(
    public val colors: HarvestCircleColors,
    public val typography: HarvestCircleTypography,
    public val shapes: HarvestCircleShapes,
    public val spacing: HarvestCircleSpacing,
)

@Immutable
public class HarvestCircleShellTokens internal constructor(
    public val layout: HarvestCircleLayout,
    public val dimensions: HarvestCircleDimensions,
)

@Immutable
public class HarvestCircleComponentTokens internal constructor(
    public val elevations: HarvestCircleElevations,
    public val motion: HarvestCircleMotion,
    public val inputMode: HarvestCircleInputMode,
)

@Immutable
internal class HarvestCircleThemeSnapshot(
    val foundation: HarvestCircleFoundationTokens,
    val shell: HarvestCircleShellTokens,
    val component: HarvestCircleComponentTokens,
    val textSelectionColors: TextSelectionColors,
)

internal fun createHarvestCircleThemeSnapshot(
    dark: Boolean,
    config: HarvestCircleThemeConfig,
    typography: HarvestCircleTypography,
): HarvestCircleThemeSnapshot {
    val colors =
        when {
            dark && config.contrast == HarvestCircleContrast.High ->
                HarvestCircleColorSchemes.DarkHighContrast
            dark -> HarvestCircleColorSchemes.Dark
            config.contrast == HarvestCircleContrast.High ->
                HarvestCircleColorSchemes.LightHighContrast
            else -> HarvestCircleColorSchemes.Light
        }
    return HarvestCircleThemeSnapshot(
        foundation =
            HarvestCircleFoundationTokens(
                colors = colors,
                typography = typography,
                shapes = HarvestCircleDefaultShapes,
                spacing = HarvestCircleDefaultSpacing,
            ),
        shell =
            HarvestCircleShellTokens(
                layout = harvestCircleLayout(config.density),
                dimensions = harvestCircleDimensions(config.density, config.inputMode),
            ),
        component =
            HarvestCircleComponentTokens(
                elevations = HarvestCircleDefaultElevations,
                motion = harvestCircleMotion(config.motion),
                inputMode = config.inputMode,
            ),
        textSelectionColors =
            TextSelectionColors(
                handleColor = colors.focus.ring,
                backgroundColor = colors.focus.selection,
            ),
    )
}
