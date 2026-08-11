package org.harvestcircle.design

@JvmInline
value class ColorToken private constructor(
    val hex: String,
) {
    companion object {
        fun parse(value: String): ColorToken {
            require(value.matches(Regex("#[0-9A-F]{6}")))
            return ColorToken(value)
        }
    }
}

data class HarvestCirclePalette(
    val background: ColorToken,
    val surface: ColorToken,
    val surfaceSecondary: ColorToken,
    val textPrimary: ColorToken,
    val textSecondary: ColorToken,
    val border: ColorToken,
    val primary: ColorToken,
    val primaryHover: ColorToken,
    val information: ColorToken,
    val positive: ColorToken,
    val caution: ColorToken,
    val critical: ColorToken,
    val focus: ColorToken,
)

enum class FontWeightToken { Regular, Semibold }

data class TypographyToken(
    val sizeSp: Int,
    val weight: FontWeightToken,
    val monospace: Boolean = false,
)

object HarvestCircleDesign {
    val light =
        palette(
            "#F4F6F4",
            "#FFFFFF",
            "#EEF2F0",
            "#17201D",
            "#56615C",
            "#D5DCDA",
            "#215E57",
            "#194B46",
            "#315F78",
            "#3E6B53",
            "#87651F",
            "#88413B",
            "#315F78",
        )
    val dark =
        palette(
            "#111714",
            "#17201D",
            "#202A26",
            "#EEF3F0",
            "#AEBAB4",
            "#34413C",
            "#65A79D",
            "#7BB9B0",
            "#7FA9C1",
            "#77A98A",
            "#D0AA57",
            "#D28A83",
            "#8EB8CF",
        )

    val screenTitle = TypographyToken(28, FontWeightToken.Semibold)
    val sectionTitle = TypographyToken(20, FontWeightToken.Semibold)
    val cardTitle = TypographyToken(17, FontWeightToken.Semibold)
    val body = TypographyToken(16, FontWeightToken.Regular)
    val secondary = TypographyToken(14, FontWeightToken.Regular)
    val protocol = TypographyToken(13, FontWeightToken.Regular, monospace = true)
    val button = TypographyToken(16, FontWeightToken.Semibold)

    val spacingDp = listOf(2, 4, 8, 12, 16, 24, 32, 40, 48, 64)
    const val SMALL_RADIUS_DP = 4
    const val CONTROL_RADIUS_DP = 8
    const val SURFACE_RADIUS_DP = 12
    const val BORDER_DP = 1
    const val MINIMUM_TARGET_DP = 44
    const val PRIMARY_CONTROL_DP = 48
}

enum class ThemePreference { System, Light, Dark }

enum class TextSizePreference(
    val scale: Float,
) {
    Default(1f),
    Large(1.15f),
    VeryLarge(1.3f),
}

enum class MotionPreference { Standard, Reduced }

data class AppearanceState(
    val theme: ThemePreference = ThemePreference.System,
    val textSize: TextSizePreference = TextSizePreference.Default,
    val motion: MotionPreference = MotionPreference.Standard,
)

private fun palette(
    background: String,
    surface: String,
    surfaceSecondary: String,
    textPrimary: String,
    textSecondary: String,
    border: String,
    primary: String,
    primaryHover: String,
    information: String,
    positive: String,
    caution: String,
    critical: String,
    focus: String,
): HarvestCirclePalette =
    listOf(
        background,
        surface,
        surfaceSecondary,
        textPrimary,
        textSecondary,
        border,
        primary,
        primaryHover,
        information,
        positive,
        caution,
        critical,
        focus,
    ).map(ColorToken::parse).let { colors ->
        HarvestCirclePalette(
            colors[0],
            colors[1],
            colors[2],
            colors[3],
            colors[4],
            colors[5],
            colors[6],
            colors[7],
            colors[8],
            colors[9],
            colors[10],
            colors[11],
            colors[12],
        )
    }
