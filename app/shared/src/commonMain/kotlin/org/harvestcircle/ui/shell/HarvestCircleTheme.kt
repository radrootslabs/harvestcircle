package org.harvestcircle.ui.shell

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.MotionPreference
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.appearance.ThemePreference
import org.harvestcircle.designsystem.theme.HarvestCircleContrast
import org.harvestcircle.designsystem.theme.HarvestCircleDensity
import org.harvestcircle.designsystem.theme.HarvestCircleDesignTheme
import org.harvestcircle.designsystem.theme.HarvestCircleInputMode
import org.harvestcircle.designsystem.theme.HarvestCircleMotionMode
import org.harvestcircle.designsystem.theme.HarvestCircleTextScale
import org.harvestcircle.designsystem.theme.HarvestCircleThemeConfig
import org.harvestcircle.designsystem.theme.HarvestCircleThemeMode

fun harvestCircleDesignThemeConfig(
    appearance: AppearanceState,
    systemDark: Boolean,
): HarvestCircleThemeConfig =
    HarvestCircleThemeConfig(
        mode =
            when (appearance.theme) {
                ThemePreference.System -> if (systemDark) HarvestCircleThemeMode.Dark else HarvestCircleThemeMode.Light
                ThemePreference.Light -> HarvestCircleThemeMode.Light
                ThemePreference.Dark -> HarvestCircleThemeMode.Dark
            },
        contrast = HarvestCircleContrast.Standard,
        density = HarvestCircleDensity.Comfortable,
        motion =
            when (appearance.motion) {
                MotionPreference.Standard -> HarvestCircleMotionMode.Full
                MotionPreference.Reduced -> HarvestCircleMotionMode.Reduced
            },
        inputMode = HarvestCircleInputMode.Pointer,
        textScale =
            when (appearance.textSize) {
                TextSizePreference.Default -> HarvestCircleTextScale.Standard
                TextSizePreference.Large -> HarvestCircleTextScale.Large
                TextSizePreference.VeryLarge -> HarvestCircleTextScale.ExtraLarge
            },
    )

@Composable
fun HarvestCircleTheme(
    appearance: AppearanceState,
    systemDark: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    HarvestCircleDesignTheme(config = harvestCircleDesignThemeConfig(appearance, systemDark)) {
        CompositionLocalProvider(
            LocalShellAppearance provides appearance,
        ) {
            content()
        }
    }
}
