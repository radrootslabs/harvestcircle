package org.harvestcircle.ui.shell

import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.MotionPreference
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.appearance.ThemePreference
import org.harvestcircle.designsystem.theme.HarvestCircleInputMode
import org.harvestcircle.designsystem.theme.HarvestCircleMotionMode
import org.harvestcircle.designsystem.theme.HarvestCircleTextScale
import org.harvestcircle.designsystem.theme.HarvestCircleThemeMode
import kotlin.test.Test
import kotlin.test.assertEquals

class HarvestCircleThemeAdapterTest {
    @Test
    fun applicationAppearanceMapsToBoundedDesignConfiguration() {
        val config =
            harvestCircleDesignThemeConfig(
                appearance =
                    AppearanceState(
                        theme = ThemePreference.System,
                        textSize = TextSizePreference.VeryLarge,
                        motion = MotionPreference.Reduced,
                    ),
                systemDark = true,
            )

        assertEquals(HarvestCircleThemeMode.Dark, config.mode)
        assertEquals(HarvestCircleTextScale.ExtraLarge, config.textScale)
        assertEquals(HarvestCircleMotionMode.Reduced, config.motion)
        assertEquals(HarvestCircleInputMode.Pointer, config.inputMode)
    }
}
