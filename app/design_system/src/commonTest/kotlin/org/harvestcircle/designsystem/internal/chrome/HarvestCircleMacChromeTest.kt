package org.harvestcircle.designsystem.internal.chrome

import androidx.compose.ui.graphics.Color
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class HarvestCircleMacChromeTest {
    @Test
    fun softFilledBorderMovesPrimaryBlueTowardWhite() {
        val container = Color(0xFF0066CC)
        val border =
            harvestCircleMacSoftFilledBorder(
                container = container,
                dark = false,
                highContrast = false,
            )

        assertTrue(border.red > container.red)
        assertTrue(border.green > container.green)
        assertTrue(border.blue > container.blue)
    }

    @Test
    fun highContrastKeepsCanonicalStrongBoundary() {
        val container = Color(0xFF0066CC)

        assertEquals(
            expected = harvestCircleMacFilledBorder(container = container, dark = false),
            actual =
                harvestCircleMacSoftFilledBorder(
                    container = container,
                    dark = false,
                    highContrast = true,
                ),
        )
    }
}
