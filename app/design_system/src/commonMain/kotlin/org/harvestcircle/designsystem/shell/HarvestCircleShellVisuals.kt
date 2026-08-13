package org.harvestcircle.designsystem.shell

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Approved source-derived semantic palette for HarvestCircle application chrome. */
@Immutable
public data class HarvestCircleShellColors(
    public val isDark: Boolean,
    public val viewportCanvas: Color,
    public val applicationFrame: Color,
    public val sidebar: Color,
    public val pane: Color,
    public val raised: Color,
    public val input: Color,
    public val navigationHover: Color,
    public val navigationPressed: Color,
    public val navigationSelected: Color,
    public val divider: Color,
    public val border: Color,
    public val contentPrimary: Color,
    public val contentSecondary: Color,
    public val contentMuted: Color,
    public val contentDisabled: Color,
    public val accent: Color,
    public val accentHover: Color,
    public val accentPressed: Color,
    public val accentSubtle: Color,
    public val onAccent: Color,
    public val destructive: Color,
    public val destructiveHover: Color,
    public val destructivePressed: Color,
    public val onDestructive: Color,
    public val shadow: Color,
)

private val HarvestCircleShellLightColors =
    HarvestCircleShellColors(
        isDark = false,
        viewportCanvas = Color(0xFFD9D9D7),
        applicationFrame = Color(0xFFFFFFFF),
        sidebar = Color(0xFFF6F6F4),
        pane = Color(0xFFFFFFFF),
        raised = Color(0xFFFFFFFF),
        input = Color(0xFFF3F3F1),
        navigationHover = Color(0xFFF0F0ED),
        navigationPressed = Color(0xFFE8E8E4),
        navigationSelected = Color(0xFFECECEA),
        divider = Color(0xFFE5E6E9),
        border = Color(0xFFE5E6E9),
        contentPrimary = Color(0xFF141517),
        contentSecondary = Color(0xFF626560),
        contentMuted = Color(0xFF858984),
        contentDisabled = Color(0xFFA7AAA6),
        accent = Color(0xFF155239),
        accentHover = Color(0xFF1B6547),
        accentPressed = Color(0xFF10432F),
        accentSubtle = Color(0xFFE6EFEA),
        onAccent = Color.White,
        destructive = Color(0xFF982018),
        destructiveHover = Color(0xFFB42318),
        destructivePressed = Color(0xFF7A1A14),
        onDestructive = Color.White,
        shadow = Color(0x2E000000),
    )

private val HarvestCircleShellDarkColors =
    HarvestCircleShellColors(
        isDark = true,
        viewportCanvas = Color(0xFF0E100F),
        applicationFrame = Color(0xFF171917),
        sidebar = Color(0xFF1B1D1B),
        pane = Color(0xFF1E201E),
        raised = Color(0xFF242624),
        input = Color(0xFF222422),
        navigationHover = Color(0xFF252825),
        navigationPressed = Color(0xFF2D302D),
        navigationSelected = Color(0xFF292C29),
        divider = Color(0xFF30332F),
        border = Color(0xFF343834),
        contentPrimary = Color(0xFFF1F2EF),
        contentSecondary = Color(0xFFB5BAB5),
        contentMuted = Color(0xFF878D87),
        contentDisabled = Color(0xFF666B66),
        accent = Color(0xFF155239),
        accentHover = Color(0xFF1D6849),
        accentPressed = Color(0xFF10432F),
        accentSubtle = Color(0xFF173126),
        onAccent = Color.White,
        destructive = Color(0xFFB42318),
        destructiveHover = Color(0xFFD92D20),
        destructivePressed = Color(0xFF982018),
        onDestructive = Color.White,
        shadow = Color(0x66000000),
    )

public fun harvestCircleShellColors(isDark: Boolean): HarvestCircleShellColors =
    if (isDark) HarvestCircleShellDarkColors else HarvestCircleShellLightColors

public val HarvestCircleShellPalette: HarvestCircleShellColors
    @Composable
    @ReadOnlyComposable
    get() = harvestCircleShellColors(HarvestCircleTheme.foundation.colors.isDark)

/** Fixed application-shell geometry derived from the approved source frames. */
public object HarvestCircleShellMetrics {
    public val topBarHeight: Dp = 48.dp
    public val localHeaderHeight: Dp = 40.dp
    public val structuralDividerWidth: Dp = 1.dp
    public val sidebarWidth: Dp = 232.dp
    public val collapsedSidebarWidth: Dp = 72.dp
    public val sidebarHorizontalInset: Dp = 16.dp
    public val sidebarHeaderHeight: Dp = 48.dp
    public val sidebarHeaderIconTarget: Dp = 28.dp
    public val sidebarHeaderIconSize: Dp = 15.dp
    public val sidebarQuickActionHeight: Dp = 32.dp
    public val sidebarQuickActionBottomGap: Dp = 8.dp
    public val sidebarSectionHeaderHeight: Dp = 28.dp
    public val sidebarSectionGap: Dp = 14.dp
    public val sidebarNavigationRowHeight: Dp = 32.dp
    public val sidebarNavigationGap: Dp = 2.dp
    public val sidebarNavigationHorizontalPadding: Dp = 8.dp
    public val sidebarNavigationIconSize: Dp = 16.dp
    public val sidebarNavigationIconGap: Dp = 10.dp
    public val sidebarFooterOuterInset: Dp = 12.dp
    public val sidebarFooterHeight: Dp = 36.dp
    public val sidebarFooterIconSize: Dp = 18.dp
    public val topBarHorizontalInset: Dp = 12.dp
    public val topBarControlGap: Dp = 8.dp
    public val topBarSquareControlSize: Dp = 32.dp
    public val topBarIconSize: Dp = 16.dp
    public val topBarSearchWidth: Dp = 280.dp
    public val topBarSearchCompactWidth: Dp = 176.dp
    public val localHeaderHorizontalInset: Dp = 12.dp
    public val localHeaderActionSize: Dp = 28.dp
    public val localHeaderGap: Dp = 8.dp
    public val controlRadius: Dp = 10.dp
    public val navigationRadius: Dp = 8.dp
    public val contentPageHorizontalInset: Dp = 24.dp
    public val contentPageVerticalInset: Dp = 24.dp
    public val emptyStateWidth: Dp = 344.dp
    public val emptyStateIllustrationWidth: Dp = 56.dp
    public val emptyStateIllustrationHeight: Dp = 64.dp
    public val emptyStateIllustrationStrokeWidth: Dp = 1.1.dp
    public val emptyStateIllustrationToTitleGap: Dp = 24.dp
    public val emptyStateTitleToBodyGap: Dp = 12.dp
    public val emptyStateParagraphGap: Dp = 12.dp
    public val emptyStateBodyToActionsGap: Dp = 20.dp
    public val emptyStateActionGap: Dp = 12.dp
    public val emptyStateVerticalOffset: Dp = 8.dp
    public val contentPanelRadius: Dp = 12.dp
    public val contentPanelInset: Dp = 16.dp
    public val canvasActionBarHeight: Dp = 64.dp
    public val canvasContentMaxWidth: Dp = 760.dp
    public val compactBreakpoint: Dp = 976.dp
    public val expandedBreakpoint: Dp = 1272.dp
}
