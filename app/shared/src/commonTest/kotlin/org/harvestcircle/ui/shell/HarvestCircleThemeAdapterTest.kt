package org.harvestcircle.ui.shell

import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.design.ThemePreference
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
