package org.harvestcircle.designsystem.theme

import androidx.compose.ui.graphics.Color
import org.harvestcircle.designsystem.theme.color.HarvestCircleColorSchemes
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue

class HarvestCircleTokenInvariantTest {
    @Test
    fun semanticColorsAreSpecified() {
        val schemes =
            listOf(
                HarvestCircleColorSchemes.Light,
                HarvestCircleColorSchemes.Dark,
                HarvestCircleColorSchemes.LightHighContrast,
                HarvestCircleColorSchemes.DarkHighContrast,
            )

        schemes.forEach { colors ->
            listOf(
                colors.surface.canvas,
                colors.surface.base,
                colors.surface.raised,
                colors.surface.sunken,
                colors.surface.selected,
                colors.content.primary,
                colors.content.secondary,
                colors.content.muted,
                colors.border.subtle,
                colors.border.default,
                colors.border.strong,
                colors.action.primary.rest,
                colors.action.primary.content,
                colors.action.destructive.rest,
                colors.focus.ring,
                colors.focus.selection,
            ).forEach { color ->
                assertNotEquals(Color.Unspecified, color)
            }
        }
    }

    @Test
    fun visualAndInteractionDimensionsRemainIntentional() {
        HarvestCircleDensity.entries.forEach { density ->
            val pointer = harvestCircleDimensions(density, HarvestCircleInputMode.Pointer)
            val touch = harvestCircleDimensions(density, HarvestCircleInputMode.Touch)

            assertTrue(pointer.minimumInteractive.value >= 28f)
            assertTrue(touch.minimumInteractive.value >= 48f)
            assertTrue(pointer.controlSmall.value > 0f)
            assertTrue(pointer.controlMedium.value >= pointer.controlSmall.value)
            assertTrue(pointer.controlLarge.value >= pointer.controlMedium.value)
            assertTrue(pointer.controlLarge.value <= 32f)
            assertTrue(pointer.focusRingWidth.value >= 2f)
            assertTrue(pointer.selectionControl.value in 13f..14f)
            assertTrue(pointer.switchHeight.value in 16f..18f)
        }
    }

    @Test
    fun typographyLineHeightsDoNotUndercutFontSizes() {
        val typography = createHarvestCircleTypography()

        listOf(
            typography.display,
            typography.pageTitle,
            typography.sectionTitle,
            typography.subsectionTitle,
            typography.body,
            typography.bodyStrong,
            typography.bodySmall,
            typography.label,
            typography.labelSmall,
            typography.code,
        ).forEach { style ->
            assertTrue(style.lineHeight.value >= style.fontSize.value)
        }
    }

    @Test
    fun productTextScalesAreBoundedAndDeterministic() {
        val standard = createHarvestCircleTypography(scale = HarvestCircleTextScale.Standard.factor)
        val large = createHarvestCircleTypography(scale = HarvestCircleTextScale.Large.factor)
        val extraLarge =
            createHarvestCircleTypography(scale = HarvestCircleTextScale.ExtraLarge.factor)

        assertEquals(13F, standard.body.fontSize.value)
        assertEquals(14.95F, large.body.fontSize.value, absoluteTolerance = 0.001F)
        assertEquals(16.9F, extraLarge.body.fontSize.value, absoluteTolerance = 0.001F)
    }
}
