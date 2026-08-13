package org.harvestcircle.designsystem.theme

import androidx.compose.ui.graphics.Color
import org.harvestcircle.designsystem.theme.color.HarvestCircleColorSchemes
import org.harvestcircle.designsystem.theme.color.HarvestCircleColors
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.test.Test
import kotlin.test.assertTrue

class HarvestCircleColorContrastTest {
    private val schemes =
        listOf(
            HarvestCircleColorSchemes.Light,
            HarvestCircleColorSchemes.Dark,
            HarvestCircleColorSchemes.LightHighContrast,
            HarvestCircleColorSchemes.DarkHighContrast,
        )

    @Test
    fun primaryContentMeetsNormalTextContrast() {
        schemes.forEach { colors ->
            assertContrast(
                foreground = colors.content.primary,
                background = colors.surface.canvas,
                minimum = 4.5,
                label = "${colors.description()} primary content on canvas",
            )
            assertContrast(
                foreground = colors.content.primary,
                background = colors.surface.base,
                minimum = 4.5,
                label = "${colors.description()} primary content on base",
            )
        }
    }

    @Test
    fun hcSc009PrimaryAndDestructiveActionsMeetNormalTextContrast() {
        schemes.forEach { colors ->
            assertContrast(
                foreground = colors.action.primary.content,
                background = colors.action.primary.rest,
                minimum = 4.5,
                label = "${colors.description()} primary action",
            )
            assertContrast(
                foreground = colors.action.destructive.content,
                background = colors.action.destructive.rest,
                minimum = 4.5,
                label = "${colors.description()} destructive action",
            )
        }
    }

    @Test
    fun subtleFeedbackContentMeetsNormalTextContrast() {
        schemes.forEach { colors ->
            listOf(
                "info" to colors.feedback.info,
                "success" to colors.feedback.success,
                "warning" to colors.feedback.warning,
                "error" to colors.feedback.error,
            ).forEach { (name, role) ->
                assertContrast(
                    foreground = role.onSubtle,
                    background = role.subtle,
                    minimum = 4.5,
                    label = "${colors.description()} $name subtle feedback",
                )
            }
        }
    }

    @Test
    fun hcSc008FocusRingSeparatesFromPrimarySurfaces() {
        schemes.forEach { colors ->
            assertContrast(
                foreground = colors.focus.ring,
                background = colors.surface.base,
                minimum = 3.0,
                label = "${colors.description()} focus ring on base",
            )
            assertContrast(
                foreground = colors.focus.ring,
                background = colors.surface.canvas,
                minimum = 3.0,
                label = "${colors.description()} focus ring on canvas",
            )
        }
    }
}

private fun HarvestCircleColors.description(): String =
    buildString {
        append(if (isDark) "dark" else "light")
        append(if (isHighContrast) " high contrast" else " standard")
    }

private fun assertContrast(
    foreground: Color,
    background: Color,
    minimum: Double,
    label: String,
) {
    val actual = contrastRatio(foreground, background)
    assertTrue(
        actual >= minimum,
        "$label contrast was ${actual.formatTwoDecimals()}, expected at least $minimum",
    )
}

private fun contrastRatio(
    first: Color,
    second: Color,
): Double {
    val firstLuminance = first.relativeLuminance()
    val secondLuminance = second.relativeLuminance()
    return (max(firstLuminance, secondLuminance) + 0.05) /
        (min(firstLuminance, secondLuminance) + 0.05)
}

private fun Color.relativeLuminance(): Double =
    0.2126 * red.toDouble().linearized() +
        0.7152 * green.toDouble().linearized() +
        0.0722 * blue.toDouble().linearized()

private fun Double.linearized(): Double =
    if (this <= 0.04045) {
        this / 12.92
    } else {
        ((this + 0.055) / 1.055).pow(2.4)
    }

private fun Double.formatTwoDecimals(): String = ((this * 100.0).toInt() / 100.0).toString()
