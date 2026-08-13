package org.harvestcircle.designsystem.theme

import androidx.compose.runtime.Immutable
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** A small, intentional spacing scale for local composition. */
@Immutable
public class HarvestCircleSpacing internal constructor(
    public val none: Dp,
    public val xxs: Dp,
    public val xs: Dp,
    public val sm: Dp,
    public val md: Dp,
    public val lg: Dp,
    public val xl: Dp,
    public val xxl: Dp,
    public val xxxl: Dp,
)

/** Semantic spacing for repeated product structure. */
@Immutable
public class HarvestCircleLayout internal constructor(
    public val pageInset: Dp,
    public val paneInset: Dp,
    public val sectionGap: Dp,
    public val contentGap: Dp,
    public val fieldGap: Dp,
    public val inlineGap: Dp,
)

/**
 * Canonical control and application dimensions.
 *
 * Visual control sizes follow the compact AppKit metrics used from Big Sur through Sequoia. The
 * interactive target is separately expanded for touch hosts through [HarvestCircleInputMode].
 */
@Immutable
public class HarvestCircleDimensions internal constructor(
    public val minimumInteractive: Dp,
    public val controlSmall: Dp,
    public val controlMedium: Dp,
    public val controlLarge: Dp,
    public val iconSmall: Dp,
    public val iconMedium: Dp,
    public val iconLarge: Dp,
    public val toolbarHeight: Dp,
    public val rowHeight: Dp,
    public val focusRingWidth: Dp,
    public val focusRingGap: Dp,
    public val dividerWidth: Dp,
    public val selectionControl: Dp,
    public val switchWidth: Dp,
    public val switchHeight: Dp,
    public val menuItemHeight: Dp,
)

/** Reference-derived geometry for the desktop application frame. */
@Immutable
public class HarvestCircleFrameMetrics internal constructor(
    public val structuralDividerWidth: Dp,
    public val topBarHeight: Dp,
    public val localHeaderHeight: Dp,
    public val sidebarWidth: Dp,
    public val collapsedSidebarWidth: Dp,
    public val narrowPaneWidth: Dp,
    public val standardPaneWidth: Dp,
    public val widePaneWidth: Dp,
    public val utilityPaneWidth: Dp,
    public val inspectorPaneWidth: Dp,
    public val mainPaneMinimumWidth: Dp,
    public val compactBreakpoint: Dp,
    public val expandedBreakpoint: Dp,
)

/** Shadow elevations. Surfaces remain mostly flat, as in canonical AppKit windows. */
@Immutable
public class HarvestCircleElevations internal constructor(
    public val flat: Dp,
    public val raised: Dp,
    public val overlay: Dp,
    public val dialog: Dp,
)

/** Animation durations in milliseconds. */
@Immutable
public class HarvestCircleMotion internal constructor(
    public val immediateMillis: Int,
    public val quickMillis: Int,
    public val standardMillis: Int,
    public val deliberateMillis: Int,
)

internal val HarvestCircleDefaultSpacing: HarvestCircleSpacing =
    HarvestCircleSpacing(
        none = 0.dp,
        xxs = 2.dp,
        xs = 4.dp,
        sm = 6.dp,
        md = 8.dp,
        lg = 12.dp,
        xl = 16.dp,
        xxl = 24.dp,
        xxxl = 32.dp,
    )

internal fun harvestCircleLayout(density: HarvestCircleDensity): HarvestCircleLayout =
    when (density) {
        HarvestCircleDensity.Compact ->
            HarvestCircleLayout(
                pageInset = 16.dp,
                paneInset = 12.dp,
                sectionGap = 18.dp,
                contentGap = 10.dp,
                fieldGap = 8.dp,
                inlineGap = 6.dp,
            )

        HarvestCircleDensity.Comfortable ->
            HarvestCircleLayout(
                pageInset = 20.dp,
                paneInset = 16.dp,
                sectionGap = 24.dp,
                contentGap = 12.dp,
                fieldGap = 10.dp,
                inlineGap = 8.dp,
            )
    }

internal fun harvestCircleDimensions(
    density: HarvestCircleDensity,
    inputMode: HarvestCircleInputMode,
): HarvestCircleDimensions {
    val minimumInteractive =
        when (inputMode) {
            HarvestCircleInputMode.Pointer -> if (density == HarvestCircleDensity.Compact) 28.dp else 32.dp
            HarvestCircleInputMode.Touch -> 48.dp
        }

    return when (density) {
        HarvestCircleDensity.Compact ->
            HarvestCircleDimensions(
                minimumInteractive = minimumInteractive,
                controlSmall = 20.dp,
                controlMedium = 24.dp,
                controlLarge = 28.dp,
                iconSmall = 12.dp,
                iconMedium = 15.dp,
                iconLarge = 18.dp,
                toolbarHeight = 38.dp,
                rowHeight = 26.dp,
                focusRingWidth = 2.dp,
                focusRingGap = 2.dp,
                dividerWidth = 1.dp,
                selectionControl = 13.dp,
                switchWidth = 28.dp,
                switchHeight = 16.dp,
                menuItemHeight = 22.dp,
            )

        HarvestCircleDensity.Comfortable ->
            HarvestCircleDimensions(
                minimumInteractive = minimumInteractive,
                controlSmall = 22.dp,
                controlMedium = 28.dp,
                controlLarge = 32.dp,
                iconSmall = 13.dp,
                iconMedium = 16.dp,
                iconLarge = 20.dp,
                toolbarHeight = 44.dp,
                rowHeight = 30.dp,
                focusRingWidth = 2.dp,
                focusRingGap = 2.dp,
                dividerWidth = 1.dp,
                selectionControl = 14.dp,
                switchWidth = 32.dp,
                switchHeight = 18.dp,
                menuItemHeight = 24.dp,
            )
    }
}

internal val HarvestCircleDefaultElevations: HarvestCircleElevations =
    HarvestCircleElevations(
        flat = 0.dp,
        raised = 1.dp,
        overlay = 10.dp,
        dialog = 18.dp,
    )

internal val HarvestCircleDefaultFrameMetrics: HarvestCircleFrameMetrics =
    HarvestCircleFrameMetrics(
        structuralDividerWidth = 1.dp,
        topBarHeight = 48.dp,
        localHeaderHeight = 40.dp,
        sidebarWidth = 232.dp,
        collapsedSidebarWidth = 72.dp,
        narrowPaneWidth = 168.dp,
        standardPaneWidth = 296.dp,
        widePaneWidth = 344.dp,
        utilityPaneWidth = 296.dp,
        inspectorPaneWidth = 320.dp,
        mainPaneMinimumWidth = 480.dp,
        compactBreakpoint = 976.dp,
        expandedBreakpoint = 1272.dp,
    )

internal fun harvestCircleMotion(mode: HarvestCircleMotionMode): HarvestCircleMotion =
    when (mode) {
        HarvestCircleMotionMode.Full ->
            HarvestCircleMotion(
                immediateMillis = 0,
                quickMillis = 70,
                standardMillis = 140,
                deliberateMillis = 220,
            )

        HarvestCircleMotionMode.Reduced ->
            HarvestCircleMotion(
                immediateMillis = 0,
                quickMillis = 0,
                standardMillis = 0,
                deliberateMillis = 0,
            )
    }
