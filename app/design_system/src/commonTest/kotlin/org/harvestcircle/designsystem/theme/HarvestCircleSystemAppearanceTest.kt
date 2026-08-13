package org.harvestcircle.designsystem.theme

import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class HarvestCircleSystemAppearanceTest {
    @Test
    fun systemModeTracksObservedLightAppearance() {
        assertFalse(
            resolveHarvestCircleDarkTheme(
                mode = HarvestCircleThemeMode.System,
                systemDark = false,
            ),
        )
    }

    @Test
    fun systemModeTracksObservedDarkAppearance() {
        assertTrue(
            resolveHarvestCircleDarkTheme(
                mode = HarvestCircleThemeMode.System,
                systemDark = true,
            ),
        )
    }

    @Test
    fun fixedLightModeIgnoresDarkSystemAppearance() {
        assertFalse(
            resolveHarvestCircleDarkTheme(
                mode = HarvestCircleThemeMode.Light,
                systemDark = true,
            ),
        )
    }

    @Test
    fun fixedDarkModeIgnoresLightSystemAppearance() {
        assertTrue(
            resolveHarvestCircleDarkTheme(
                mode = HarvestCircleThemeMode.Dark,
                systemDark = false,
            ),
        )
    }
}
