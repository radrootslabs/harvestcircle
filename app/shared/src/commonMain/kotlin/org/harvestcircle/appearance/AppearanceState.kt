package org.harvestcircle.appearance

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
