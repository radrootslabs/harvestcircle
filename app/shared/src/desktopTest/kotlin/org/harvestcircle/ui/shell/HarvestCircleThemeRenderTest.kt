package org.harvestcircle.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.ThemePreference
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import org.harvestcircle.designsystem.theme.HarvestCircleTheme as DesignTheme

@OptIn(ExperimentalTestApi::class)
class HarvestCircleThemeRenderTest {
    @Test
    fun lightDarkAndSystemPreferencesChangeRenderedPixels() =
        runComposeUiTest {
            var preference by mutableStateOf(ThemePreference.Light)
            var systemDark by mutableStateOf(false)
            setHarvestCircleContent {
                HarvestCircleTheme(AppearanceState(theme = preference), systemDark = systemDark) {
                    Box(
                        Modifier
                            .size(32.dp)
                            .background(DesignTheme.foundation.colors.surface.canvas)
                            .testTag("theme-render-swatch"),
                    )
                }
            }

            fun renderedColor() = onNodeWithTag("theme-render-swatch").captureToImage().toPixelMap()[16, 16]

            val explicitLight = renderedColor()
            preference = ThemePreference.Dark
            waitForIdle()
            val explicitDark = renderedColor()
            preference = ThemePreference.System
            systemDark = false
            waitForIdle()
            val systemLight = renderedColor()
            systemDark = true
            waitForIdle()
            val systemDarkColor = renderedColor()

            assertEquals(explicitLight, systemLight)
            assertEquals(explicitDark, systemDarkColor)
            assertNotEquals(explicitLight, explicitDark)
        }
}
