package org.harvestcircle.design

import org.harvestcircle.ui.shell.ResolvedTheme
import org.harvestcircle.ui.shell.harvestCircleThemeTokens
import org.harvestcircle.ui.shell.resolveTheme
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFails
import kotlin.test.assertTrue

class HarvestCircleDesignTest {
    @Test
    fun palettesMatchTheLockedNordicTokens() {
        assertEquals("#F4F6F4", HarvestCircleDesign.light.background.hex)
        assertEquals("#215E57", HarvestCircleDesign.light.primary.hex)
        assertEquals("#88413B", HarvestCircleDesign.light.critical.hex)
        assertEquals("#111714", HarvestCircleDesign.dark.background.hex)
        assertEquals("#65A79D", HarvestCircleDesign.dark.primary.hex)
        assertEquals("#8EB8CF", HarvestCircleDesign.dark.focus.hex)
        assertFails { ColorToken.parse("#fff") }
    }

    @Test
    fun typographySpacingAndTargetsAreCentralized() {
        assertEquals(TypographyToken(28, FontWeightToken.Semibold), HarvestCircleDesign.screenTitle)
        assertTrue(HarvestCircleDesign.protocol.monospace)
        assertEquals(listOf(2, 4, 8, 12, 16, 24, 32, 40, 48, 64), HarvestCircleDesign.spacingDp)
        assertEquals(44, HarvestCircleDesign.MINIMUM_TARGET_DP)
    }

    @Test
    fun appearanceIsSessionLocalImmutableState() {
        val defaults = AppearanceState()
        val changed =
            defaults.copy(
                theme = ThemePreference.Dark,
                textSize = TextSizePreference.VeryLarge,
                motion = MotionPreference.Reduced,
            )
        assertEquals(AppearanceState(), defaults)
        assertEquals(1.3f, changed.textSize.scale)
        assertEquals(MotionPreference.Reduced, changed.motion)
    }

    @Test
    fun themeResolutionFollowsExplicitAndInjectedSystemPreference() {
        assertEquals(ResolvedTheme.Light, resolveTheme(ThemePreference.System, systemDark = false))
        assertEquals(ResolvedTheme.Dark, resolveTheme(ThemePreference.System, systemDark = true))
        assertEquals(ResolvedTheme.Light, resolveTheme(ThemePreference.Light, systemDark = true))
        assertEquals(ResolvedTheme.Dark, resolveTheme(ThemePreference.Dark, systemDark = false))

        val light = harvestCircleThemeTokens(AppearanceState(theme = ThemePreference.Light), systemDark = true)
        val dark = harvestCircleThemeTokens(AppearanceState(theme = ThemePreference.Dark), systemDark = false)
        assertEquals(HarvestCircleDesign.light.background, light.palette.background)
        assertEquals(HarvestCircleDesign.dark.background, dark.palette.background)
    }

    @Test
    fun themeCentralizesTextScaleShapesTypographyAndMotion() {
        val tokens =
            harvestCircleThemeTokens(
                AppearanceState(
                    textSize = TextSizePreference.VeryLarge,
                    motion = MotionPreference.Reduced,
                ),
                systemDark = false,
            )
        assertEquals(1.3f, tokens.textScale)
        assertEquals(HarvestCircleDesign.screenTitle, tokens.typography.screenTitle)
        assertEquals(HarvestCircleDesign.SURFACE_RADIUS_DP, tokens.shapes.surfaceRadiusDp)
        assertEquals(false, tokens.motion.nonessentialEnabled)
    }
}
