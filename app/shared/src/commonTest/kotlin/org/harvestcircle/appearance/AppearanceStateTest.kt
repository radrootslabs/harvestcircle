package org.harvestcircle.appearance

import kotlin.test.Test
import kotlin.test.assertEquals

class AppearanceStateTest {
    @Test
    fun preferencesAreImmutableSessionState() {
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
