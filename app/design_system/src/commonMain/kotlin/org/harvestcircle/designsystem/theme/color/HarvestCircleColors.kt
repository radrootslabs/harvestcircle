package org.harvestcircle.designsystem.theme.color

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color

/**
 * Background and container colors. Names express hierarchy rather than a physical palette.
 */
@Immutable
public class HarvestCircleSurfaceColors internal constructor(
    public val canvas: Color,
    public val base: Color,
    public val raised: Color,
    public val sunken: Color,
    public val selected: Color,
    public val overlay: Color,
    public val scrim: Color,
)

/** Text and icon colors. */
@Immutable
public class HarvestCircleContentColors internal constructor(
    public val primary: Color,
    public val secondary: Color,
    public val muted: Color,
    public val disabled: Color,
    public val inverse: Color,
)

/** Boundary colors, ordered by visual emphasis. */
@Immutable
public class HarvestCircleBorderColors internal constructor(
    public val subtle: Color,
    public val default: Color,
    public val strong: Color,
    public val selected: Color,
    public val error: Color,
)

/** Colors for every state of an action treatment. */
@Immutable
public class HarvestCircleActionStateColors internal constructor(
    public val rest: Color,
    public val hover: Color,
    public val pressed: Color,
    public val disabled: Color,
    public val content: Color,
    public val disabledContent: Color,
    public val border: Color,
)

/** Canonical action treatments. */
@Immutable
public class HarvestCircleActionColors internal constructor(
    public val primary: HarvestCircleActionStateColors,
    public val secondary: HarvestCircleActionStateColors,
    public val ghost: HarvestCircleActionStateColors,
    public val destructive: HarvestCircleActionStateColors,
)

/** A strong and subtle treatment for a feedback role. */
@Immutable
public class HarvestCircleFeedbackRoleColors internal constructor(
    public val strong: Color,
    public val onStrong: Color,
    public val subtle: Color,
    public val onSubtle: Color,
    public val border: Color,
)

/** Semantic status colors. */
@Immutable
public class HarvestCircleFeedbackColors internal constructor(
    public val info: HarvestCircleFeedbackRoleColors,
    public val success: HarvestCircleFeedbackRoleColors,
    public val warning: HarvestCircleFeedbackRoleColors,
    public val error: HarvestCircleFeedbackRoleColors,
)

/** Selection and keyboard-focus colors. */
@Immutable
public class HarvestCircleFocusColors internal constructor(
    public val ring: Color,
    public val selection: Color,
    public val selectionContent: Color,
)

/**
 * The public, read-only semantic color contract. Physical palette values remain internal.
 */
@Immutable
public class HarvestCircleColors internal constructor(
    public val isDark: Boolean,
    public val isHighContrast: Boolean,
    public val surface: HarvestCircleSurfaceColors,
    public val content: HarvestCircleContentColors,
    public val border: HarvestCircleBorderColors,
    public val action: HarvestCircleActionColors,
    public val feedback: HarvestCircleFeedbackColors,
    public val focus: HarvestCircleFocusColors,
)

internal object HarvestCirclePrimitiveColors {
    val Transparent = Color.Transparent

    // Warm-neutral light surfaces.
    val WarmWhite = Color(0xFFFFFEFC)
    val Canvas = Color(0xFFF6F6F3)
    val Surface = Color(0xFFFCFCFA)
    val SurfaceSunken = Color(0xFFEEEEEA)
    val Neutral100 = Color(0xFFE5E5E0)
    val Neutral200 = Color(0xFFD7D7D1)
    val Neutral300 = Color(0xFFC4C4BD)
    val Neutral500 = Color(0xFF8B8B84)
    val Neutral600 = Color(0xFF6A6A64)
    val Neutral700 = Color(0xFF4A4A45)
    val Neutral800 = Color(0xFF30302D)
    val Neutral900 = Color(0xFF1C1C1A)
    val Black = Color(0xFF000000)
    val White = Color(0xFFFFFFFF)

    // Dark surfaces stay neutral instead of adopting a blue cast.
    val DarkCanvas = Color(0xFF101110)
    val DarkBase = Color(0xFF1A1B1A)
    val DarkRaised = Color(0xFF252625)
    val DarkSunken = Color(0xFF0B0C0B)
    val DarkBorderSubtle = Color(0xFF353633)
    val DarkBorder = Color(0xFF4B4C48)
    val DarkBorderStrong = Color(0xFF777972)
    val DarkContentPrimary = Color(0xFFF6F6F2)
    val DarkContentSecondary = Color(0xFFD6D6D0)
    val DarkContentMuted = Color(0xFFACACA5)
    val DarkContentDisabled = Color(0xFF70706B)

    // An accessible, restrained system-blue family.
    val Blue50 = Color(0xFFE8F2FC)
    val Blue100 = Color(0xFFD6E9FC)
    val Blue400 = Color(0xFF4A9BEA)
    val Blue500 = Color(0xFF0066CC)
    val Blue600 = Color(0xFF0059B3)
    val Blue700 = Color(0xFF004C99)
    val Blue800 = Color(0xFF003C7A)
    val Blue900 = Color(0xFF002B59)
    val BlueFocusDark = Color(0xFF69B4FF)
    val BlueDarkSelected = Color(0xFF18334D)

    val Green50 = Color(0xFFE8F5ED)
    val Green500 = Color(0xFF1F7A4D)
    val Green700 = Color(0xFF135C38)
    val GreenDark = Color(0xFF4FB987)
    val GreenDarkSurface = Color(0xFF123426)

    val Amber50 = Color(0xFFFFF4D6)
    val Amber500 = Color(0xFF8A5A00)
    val Amber700 = Color(0xFF634000)
    val AmberDark = Color(0xFFF2BA57)
    val AmberDarkSurface = Color(0xFF3B2A0B)

    val Red50 = Color(0xFFFDECEA)
    val Red500 = Color(0xFFB42318)
    val Red600 = Color(0xFF982018)
    val Red700 = Color(0xFF7A1A14)
    val RedDark = Color(0xFFFF766F)
    val RedDarkSurface = Color(0xFF401A18)

    val InfoDarkSurface = Color(0xFF122D46)
}

private fun action(
    rest: Color,
    hover: Color,
    pressed: Color,
    disabled: Color,
    content: Color,
    disabledContent: Color,
    border: Color,
): HarvestCircleActionStateColors =
    HarvestCircleActionStateColors(
        rest = rest,
        hover = hover,
        pressed = pressed,
        disabled = disabled,
        content = content,
        disabledContent = disabledContent,
        border = border,
    )

private fun feedback(
    strong: Color,
    onStrong: Color,
    subtle: Color,
    onSubtle: Color,
    border: Color,
): HarvestCircleFeedbackRoleColors =
    HarvestCircleFeedbackRoleColors(
        strong = strong,
        onStrong = onStrong,
        subtle = subtle,
        onSubtle = onSubtle,
        border = border,
    )

internal object HarvestCircleColorSchemes {
    val Light: HarvestCircleColors =
        HarvestCircleColors(
            isDark = false,
            isHighContrast = false,
            surface =
                HarvestCircleSurfaceColors(
                    canvas = HarvestCirclePrimitiveColors.Canvas,
                    base = HarvestCirclePrimitiveColors.Surface,
                    raised = HarvestCirclePrimitiveColors.White,
                    sunken = HarvestCirclePrimitiveColors.SurfaceSunken,
                    selected = HarvestCirclePrimitiveColors.Blue50,
                    overlay = HarvestCirclePrimitiveColors.White,
                    scrim = Color(0x66000000),
                ),
            content =
                HarvestCircleContentColors(
                    primary = HarvestCirclePrimitiveColors.Neutral900,
                    secondary = HarvestCirclePrimitiveColors.Neutral700,
                    muted = HarvestCirclePrimitiveColors.Neutral600,
                    disabled = HarvestCirclePrimitiveColors.Neutral500,
                    inverse = HarvestCirclePrimitiveColors.White,
                ),
            border =
                HarvestCircleBorderColors(
                    subtle = HarvestCirclePrimitiveColors.Neutral100,
                    default = HarvestCirclePrimitiveColors.Neutral300,
                    strong = HarvestCirclePrimitiveColors.Neutral500,
                    selected = HarvestCirclePrimitiveColors.Blue500,
                    error = HarvestCirclePrimitiveColors.Red500,
                ),
            action =
                HarvestCircleActionColors(
                    primary =
                        action(
                            rest = HarvestCirclePrimitiveColors.Blue500,
                            hover = HarvestCirclePrimitiveColors.Blue600,
                            pressed = HarvestCirclePrimitiveColors.Blue700,
                            disabled = HarvestCirclePrimitiveColors.Neutral200,
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = HarvestCirclePrimitiveColors.Neutral600,
                            border = HarvestCirclePrimitiveColors.Blue500,
                        ),
                    secondary =
                        action(
                            rest = HarvestCirclePrimitiveColors.White,
                            hover = HarvestCirclePrimitiveColors.Canvas,
                            pressed = HarvestCirclePrimitiveColors.SurfaceSunken,
                            disabled = HarvestCirclePrimitiveColors.Surface,
                            content = HarvestCirclePrimitiveColors.Neutral900,
                            disabledContent = HarvestCirclePrimitiveColors.Neutral500,
                            border = HarvestCirclePrimitiveColors.Neutral300,
                        ),
                    ghost =
                        action(
                            rest = HarvestCirclePrimitiveColors.Transparent,
                            hover = HarvestCirclePrimitiveColors.SurfaceSunken,
                            pressed = HarvestCirclePrimitiveColors.Neutral100,
                            disabled = HarvestCirclePrimitiveColors.Transparent,
                            content = HarvestCirclePrimitiveColors.Neutral900,
                            disabledContent = HarvestCirclePrimitiveColors.Neutral500,
                            border = HarvestCirclePrimitiveColors.Transparent,
                        ),
                    destructive =
                        action(
                            rest = HarvestCirclePrimitiveColors.Red500,
                            hover = HarvestCirclePrimitiveColors.Red600,
                            pressed = HarvestCirclePrimitiveColors.Red700,
                            disabled = HarvestCirclePrimitiveColors.Neutral200,
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = HarvestCirclePrimitiveColors.Neutral600,
                            border = HarvestCirclePrimitiveColors.Red500,
                        ),
                ),
            feedback =
                HarvestCircleFeedbackColors(
                    info =
                        feedback(
                            strong = HarvestCirclePrimitiveColors.Blue500,
                            onStrong = HarvestCirclePrimitiveColors.White,
                            subtle = HarvestCirclePrimitiveColors.Blue50,
                            onSubtle = HarvestCirclePrimitiveColors.Blue800,
                            border = HarvestCirclePrimitiveColors.Blue400,
                        ),
                    success =
                        feedback(
                            strong = HarvestCirclePrimitiveColors.Green500,
                            onStrong = HarvestCirclePrimitiveColors.White,
                            subtle = HarvestCirclePrimitiveColors.Green50,
                            onSubtle = HarvestCirclePrimitiveColors.Green700,
                            border = HarvestCirclePrimitiveColors.Green500,
                        ),
                    warning =
                        feedback(
                            strong = HarvestCirclePrimitiveColors.Amber500,
                            onStrong = HarvestCirclePrimitiveColors.White,
                            subtle = HarvestCirclePrimitiveColors.Amber50,
                            onSubtle = HarvestCirclePrimitiveColors.Amber700,
                            border = HarvestCirclePrimitiveColors.Amber500,
                        ),
                    error =
                        feedback(
                            strong = HarvestCirclePrimitiveColors.Red500,
                            onStrong = HarvestCirclePrimitiveColors.White,
                            subtle = HarvestCirclePrimitiveColors.Red50,
                            onSubtle = HarvestCirclePrimitiveColors.Red700,
                            border = HarvestCirclePrimitiveColors.Red500,
                        ),
                ),
            focus =
                HarvestCircleFocusColors(
                    ring = HarvestCirclePrimitiveColors.Blue500,
                    selection = HarvestCirclePrimitiveColors.Blue100,
                    selectionContent = HarvestCirclePrimitiveColors.Neutral900,
                ),
        )

    val Dark: HarvestCircleColors =
        HarvestCircleColors(
            isDark = true,
            isHighContrast = false,
            surface =
                HarvestCircleSurfaceColors(
                    canvas = HarvestCirclePrimitiveColors.DarkCanvas,
                    base = HarvestCirclePrimitiveColors.DarkBase,
                    raised = HarvestCirclePrimitiveColors.DarkRaised,
                    sunken = HarvestCirclePrimitiveColors.DarkSunken,
                    selected = HarvestCirclePrimitiveColors.BlueDarkSelected,
                    overlay = HarvestCirclePrimitiveColors.DarkRaised,
                    scrim = Color(0x99000000),
                ),
            content =
                HarvestCircleContentColors(
                    primary = HarvestCirclePrimitiveColors.DarkContentPrimary,
                    secondary = HarvestCirclePrimitiveColors.DarkContentSecondary,
                    muted = HarvestCirclePrimitiveColors.DarkContentMuted,
                    disabled = HarvestCirclePrimitiveColors.DarkContentDisabled,
                    inverse = HarvestCirclePrimitiveColors.Neutral900,
                ),
            border =
                HarvestCircleBorderColors(
                    subtle = HarvestCirclePrimitiveColors.DarkBorderSubtle,
                    default = HarvestCirclePrimitiveColors.DarkBorder,
                    strong = HarvestCirclePrimitiveColors.DarkBorderStrong,
                    selected = HarvestCirclePrimitiveColors.BlueFocusDark,
                    error = HarvestCirclePrimitiveColors.RedDark,
                ),
            action =
                HarvestCircleActionColors(
                    primary =
                        action(
                            rest = HarvestCirclePrimitiveColors.Blue500,
                            hover = HarvestCirclePrimitiveColors.Blue400,
                            pressed = HarvestCirclePrimitiveColors.Blue600,
                            disabled = HarvestCirclePrimitiveColors.DarkBorderSubtle,
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = HarvestCirclePrimitiveColors.DarkContentDisabled,
                            border = HarvestCirclePrimitiveColors.Blue500,
                        ),
                    secondary =
                        action(
                            rest = HarvestCirclePrimitiveColors.DarkRaised,
                            hover = Color(0xFF303230),
                            pressed = HarvestCirclePrimitiveColors.DarkBorderSubtle,
                            disabled = HarvestCirclePrimitiveColors.DarkBase,
                            content = HarvestCirclePrimitiveColors.DarkContentPrimary,
                            disabledContent = HarvestCirclePrimitiveColors.DarkContentDisabled,
                            border = HarvestCirclePrimitiveColors.DarkBorder,
                        ),
                    ghost =
                        action(
                            rest = HarvestCirclePrimitiveColors.Transparent,
                            hover = HarvestCirclePrimitiveColors.DarkRaised,
                            pressed = HarvestCirclePrimitiveColors.DarkBorderSubtle,
                            disabled = HarvestCirclePrimitiveColors.Transparent,
                            content = HarvestCirclePrimitiveColors.DarkContentPrimary,
                            disabledContent = HarvestCirclePrimitiveColors.DarkContentDisabled,
                            border = HarvestCirclePrimitiveColors.Transparent,
                        ),
                    destructive =
                        action(
                            rest = HarvestCirclePrimitiveColors.Red500,
                            hover = Color(0xFFC8362C),
                            pressed = HarvestCirclePrimitiveColors.Red600,
                            disabled = HarvestCirclePrimitiveColors.DarkBorderSubtle,
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = HarvestCirclePrimitiveColors.DarkContentDisabled,
                            border = HarvestCirclePrimitiveColors.RedDark,
                        ),
                ),
            feedback =
                HarvestCircleFeedbackColors(
                    info =
                        feedback(
                            strong = HarvestCirclePrimitiveColors.Blue400,
                            onStrong = HarvestCirclePrimitiveColors.Blue900,
                            subtle = HarvestCirclePrimitiveColors.InfoDarkSurface,
                            onSubtle = Color(0xFFB7DBFF),
                            border = HarvestCirclePrimitiveColors.Blue400,
                        ),
                    success =
                        feedback(
                            strong = HarvestCirclePrimitiveColors.GreenDark,
                            onStrong = Color(0xFF062518),
                            subtle = HarvestCirclePrimitiveColors.GreenDarkSurface,
                            onSubtle = Color(0xFFB6E5CC),
                            border = HarvestCirclePrimitiveColors.GreenDark,
                        ),
                    warning =
                        feedback(
                            strong = HarvestCirclePrimitiveColors.AmberDark,
                            onStrong = Color(0xFF2E1D00),
                            subtle = HarvestCirclePrimitiveColors.AmberDarkSurface,
                            onSubtle = Color(0xFFFFDFA1),
                            border = HarvestCirclePrimitiveColors.AmberDark,
                        ),
                    error =
                        feedback(
                            strong = HarvestCirclePrimitiveColors.RedDark,
                            onStrong = Color(0xFF310300),
                            subtle = HarvestCirclePrimitiveColors.RedDarkSurface,
                            onSubtle = Color(0xFFFFC3BF),
                            border = HarvestCirclePrimitiveColors.RedDark,
                        ),
                ),
            focus =
                HarvestCircleFocusColors(
                    ring = HarvestCirclePrimitiveColors.BlueFocusDark,
                    selection = Color(0xFF204B72),
                    selectionContent = HarvestCirclePrimitiveColors.White,
                ),
        )

    val LightHighContrast: HarvestCircleColors =
        HarvestCircleColors(
            isDark = false,
            isHighContrast = true,
            surface =
                HarvestCircleSurfaceColors(
                    canvas = HarvestCirclePrimitiveColors.White,
                    base = HarvestCirclePrimitiveColors.White,
                    raised = HarvestCirclePrimitiveColors.White,
                    sunken = Color(0xFFF0F0EC),
                    selected = Color(0xFFDCEBFA),
                    overlay = HarvestCirclePrimitiveColors.White,
                    scrim = Color(0x99000000),
                ),
            content =
                HarvestCircleContentColors(
                    primary = HarvestCirclePrimitiveColors.Black,
                    secondary = Color(0xFF1F1F1D),
                    muted = Color(0xFF41413D),
                    disabled = Color(0xFF666661),
                    inverse = HarvestCirclePrimitiveColors.White,
                ),
            border =
                HarvestCircleBorderColors(
                    subtle = Color(0xFF85857E),
                    default = Color(0xFF4C4C47),
                    strong = HarvestCirclePrimitiveColors.Black,
                    selected = Color(0xFF004F9E),
                    error = Color(0xFF86170F),
                ),
            action =
                HarvestCircleActionColors(
                    primary =
                        action(
                            rest = Color(0xFF004F9E),
                            hover = Color(0xFF003F80),
                            pressed = Color(0xFF002F61),
                            disabled = Color(0xFFD0D0CA),
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = Color(0xFF555550),
                            border = Color(0xFF003A75),
                        ),
                    secondary =
                        action(
                            rest = HarvestCirclePrimitiveColors.White,
                            hover = Color(0xFFEFEFEC),
                            pressed = Color(0xFFDFDFDA),
                            disabled = HarvestCirclePrimitiveColors.White,
                            content = HarvestCirclePrimitiveColors.Black,
                            disabledContent = Color(0xFF666661),
                            border = HarvestCirclePrimitiveColors.Black,
                        ),
                    ghost =
                        action(
                            rest = HarvestCirclePrimitiveColors.Transparent,
                            hover = Color(0xFFEFEFEC),
                            pressed = Color(0xFFDFDFDA),
                            disabled = HarvestCirclePrimitiveColors.Transparent,
                            content = HarvestCirclePrimitiveColors.Black,
                            disabledContent = Color(0xFF666661),
                            border = HarvestCirclePrimitiveColors.Transparent,
                        ),
                    destructive =
                        action(
                            rest = Color(0xFF86170F),
                            hover = Color(0xFF6E120C),
                            pressed = Color(0xFF570E09),
                            disabled = Color(0xFFD0D0CA),
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = Color(0xFF555550),
                            border = Color(0xFF68110B),
                        ),
                ),
            feedback =
                HarvestCircleFeedbackColors(
                    info =
                        feedback(
                            strong = Color(0xFF004F9E),
                            onStrong = HarvestCirclePrimitiveColors.White,
                            subtle = Color(0xFFDCEBFA),
                            onSubtle = Color(0xFF002F61),
                            border = Color(0xFF003A75),
                        ),
                    success =
                        feedback(
                            strong = Color(0xFF145F3B),
                            onStrong = HarvestCirclePrimitiveColors.White,
                            subtle = Color(0xFFDDF2E6),
                            onSubtle = Color(0xFF0A3E25),
                            border = Color(0xFF0F4D30),
                        ),
                    warning =
                        feedback(
                            strong = Color(0xFF6A4300),
                            onStrong = HarvestCirclePrimitiveColors.White,
                            subtle = Color(0xFFFFF0C4),
                            onSubtle = Color(0xFF4B2E00),
                            border = Color(0xFF543500),
                        ),
                    error =
                        feedback(
                            strong = Color(0xFF86170F),
                            onStrong = HarvestCirclePrimitiveColors.White,
                            subtle = Color(0xFFF9DEDA),
                            onSubtle = Color(0xFF570E09),
                            border = Color(0xFF68110B),
                        ),
                ),
            focus =
                HarvestCircleFocusColors(
                    ring = Color(0xFF003F80),
                    selection = Color(0xFFBBD9F6),
                    selectionContent = HarvestCirclePrimitiveColors.Black,
                ),
        )

    val DarkHighContrast: HarvestCircleColors =
        HarvestCircleColors(
            isDark = true,
            isHighContrast = true,
            surface =
                HarvestCircleSurfaceColors(
                    canvas = HarvestCirclePrimitiveColors.Black,
                    base = HarvestCirclePrimitiveColors.Black,
                    raised = Color(0xFF111210),
                    sunken = HarvestCirclePrimitiveColors.Black,
                    selected = Color(0xFF102D49),
                    overlay = Color(0xFF111210),
                    scrim = Color(0xCC000000),
                ),
            content =
                HarvestCircleContentColors(
                    primary = HarvestCirclePrimitiveColors.White,
                    secondary = Color(0xFFF1F1ED),
                    muted = Color(0xFFD0D0CA),
                    disabled = Color(0xFF9A9A94),
                    inverse = HarvestCirclePrimitiveColors.Black,
                ),
            border =
                HarvestCircleBorderColors(
                    subtle = Color(0xFF85857E),
                    default = Color(0xFFB0B0AA),
                    strong = HarvestCirclePrimitiveColors.White,
                    selected = Color(0xFF79BFFF),
                    error = Color(0xFFFF8E88),
                ),
            action =
                HarvestCircleActionColors(
                    primary =
                        action(
                            rest = Color(0xFF0062C4),
                            hover = Color(0xFF1678DA),
                            pressed = Color(0xFF004E9D),
                            disabled = Color(0xFF2B2C29),
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = Color(0xFF9A9A94),
                            border = Color(0xFF79BFFF),
                        ),
                    secondary =
                        action(
                            rest = Color(0xFF111210),
                            hover = Color(0xFF252622),
                            pressed = Color(0xFF353630),
                            disabled = HarvestCirclePrimitiveColors.Black,
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = Color(0xFF9A9A94),
                            border = HarvestCirclePrimitiveColors.White,
                        ),
                    ghost =
                        action(
                            rest = HarvestCirclePrimitiveColors.Transparent,
                            hover = Color(0xFF252622),
                            pressed = Color(0xFF353630),
                            disabled = HarvestCirclePrimitiveColors.Transparent,
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = Color(0xFF9A9A94),
                            border = HarvestCirclePrimitiveColors.Transparent,
                        ),
                    destructive =
                        action(
                            rest = Color(0xFF9E2118),
                            hover = Color(0xFFB42C22),
                            pressed = Color(0xFF7E1A13),
                            disabled = Color(0xFF2B2C29),
                            content = HarvestCirclePrimitiveColors.White,
                            disabledContent = Color(0xFF9A9A94),
                            border = Color(0xFFFF8E88),
                        ),
                ),
            feedback =
                HarvestCircleFeedbackColors(
                    info =
                        feedback(
                            strong = Color(0xFF79BFFF),
                            onStrong = Color(0xFF001B34),
                            subtle = Color(0xFF102D49),
                            onSubtle = HarvestCirclePrimitiveColors.White,
                            border = Color(0xFF79BFFF),
                        ),
                    success =
                        feedback(
                            strong = Color(0xFF72D5A6),
                            onStrong = Color(0xFF002416),
                            subtle = Color(0xFF123426),
                            onSubtle = HarvestCirclePrimitiveColors.White,
                            border = Color(0xFF72D5A6),
                        ),
                    warning =
                        feedback(
                            strong = Color(0xFFFFCC73),
                            onStrong = Color(0xFF2C1B00),
                            subtle = Color(0xFF3B2A0B),
                            onSubtle = HarvestCirclePrimitiveColors.White,
                            border = Color(0xFFFFCC73),
                        ),
                    error =
                        feedback(
                            strong = Color(0xFFFF8E88),
                            onStrong = Color(0xFF300300),
                            subtle = Color(0xFF401A18),
                            onSubtle = HarvestCirclePrimitiveColors.White,
                            border = Color(0xFFFF8E88),
                        ),
                ),
            focus =
                HarvestCircleFocusColors(
                    ring = Color(0xFF79BFFF),
                    selection = Color(0xFF244E75),
                    selectionContent = HarvestCirclePrimitiveColors.White,
                ),
        )
}
