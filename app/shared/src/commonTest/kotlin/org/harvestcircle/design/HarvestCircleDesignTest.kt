package org.harvestcircle.design

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
}
