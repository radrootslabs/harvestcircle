package org.harvestcircle.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.ColorToken
import org.harvestcircle.design.HarvestCircleDesign
import org.harvestcircle.design.HarvestCirclePalette
import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.design.TypographyToken

enum class ResolvedTheme { Light, Dark }

data class HarvestCircleTypography(
    val screenTitle: TypographyToken = HarvestCircleDesign.screenTitle,
    val sectionTitle: TypographyToken = HarvestCircleDesign.sectionTitle,
    val cardTitle: TypographyToken = HarvestCircleDesign.cardTitle,
    val body: TypographyToken = HarvestCircleDesign.body,
    val secondary: TypographyToken = HarvestCircleDesign.secondary,
    val protocol: TypographyToken = HarvestCircleDesign.protocol,
    val button: TypographyToken = HarvestCircleDesign.button,
)

data class HarvestCircleShapes(
    val smallRadiusDp: Int = HarvestCircleDesign.SMALL_RADIUS_DP,
    val controlRadiusDp: Int = HarvestCircleDesign.CONTROL_RADIUS_DP,
    val surfaceRadiusDp: Int = HarvestCircleDesign.SURFACE_RADIUS_DP,
)

data class HarvestCircleMotion(
    val nonessentialEnabled: Boolean,
)

data class HarvestCircleThemeTokens(
    val resolvedTheme: ResolvedTheme,
    val palette: HarvestCirclePalette,
    val typography: HarvestCircleTypography,
    val shapes: HarvestCircleShapes,
    val motion: HarvestCircleMotion,
    val textScale: Float,
)

val LocalHarvestCirclePalette = staticCompositionLocalOf { HarvestCircleDesign.light }
val LocalHarvestCircleTypography = staticCompositionLocalOf { HarvestCircleTypography() }
val LocalHarvestCircleShapes = staticCompositionLocalOf { HarvestCircleShapes() }
val LocalHarvestCircleMotion = staticCompositionLocalOf { HarvestCircleMotion(nonessentialEnabled = true) }

fun resolveTheme(
    preference: ThemePreference,
    systemDark: Boolean,
): ResolvedTheme =
    when (preference) {
        ThemePreference.System -> if (systemDark) ResolvedTheme.Dark else ResolvedTheme.Light
        ThemePreference.Light -> ResolvedTheme.Light
        ThemePreference.Dark -> ResolvedTheme.Dark
    }

fun harvestCircleThemeTokens(
    appearance: AppearanceState,
    systemDark: Boolean,
): HarvestCircleThemeTokens {
    val resolved = resolveTheme(appearance.theme, systemDark)
    return HarvestCircleThemeTokens(
        resolvedTheme = resolved,
        palette = if (resolved == ResolvedTheme.Dark) HarvestCircleDesign.dark else HarvestCircleDesign.light,
        typography = HarvestCircleTypography(),
        shapes = HarvestCircleShapes(),
        motion = HarvestCircleMotion(nonessentialEnabled = appearance.motion != MotionPreference.Reduced),
        textScale = appearance.textSize.scale,
    )
}

@Composable
fun HarvestCircleTheme(
    appearance: AppearanceState,
    systemDark: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val tokens = harvestCircleThemeTokens(appearance, systemDark)
    val baseDensity = LocalDensity.current
    val scaledDensity = Density(baseDensity.density, baseDensity.fontScale * tokens.textScale)
    CompositionLocalProvider(
        LocalShellAppearance provides appearance,
        LocalHarvestCirclePalette provides tokens.palette,
        LocalHarvestCircleTypography provides tokens.typography,
        LocalHarvestCircleShapes provides tokens.shapes,
        LocalHarvestCircleMotion provides tokens.motion,
        LocalDensity provides scaledDensity,
    ) {
        Box(
            Modifier
                .fillMaxSize()
                .background(tokens.palette.background.toComposeColor()),
        ) {
            content()
        }
    }
}

fun ColorToken.toComposeColor(): Color {
    val rgb = hex.removePrefix("#").toLong(16)
    return Color(rgb.toInt()).copy(alpha = 1f)
}
