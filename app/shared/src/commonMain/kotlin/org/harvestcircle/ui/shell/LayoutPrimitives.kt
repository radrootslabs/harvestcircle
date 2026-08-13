package org.harvestcircle.ui.shell

import org.harvestcircle.appearance.TextSizePreference

object ShellDimensions {
    const val PREFERRED_WINDOW_WIDTH_DP = 1280
    const val PREFERRED_WINDOW_HEIGHT_DP = 800
    const val MINIMUM_WINDOW_WIDTH_DP = 1100
    const val MINIMUM_WINDOW_HEIGHT_DP = 720
    const val SIDEBAR_WIDTH_DP = 232
    const val GLOBAL_TOP_BAR_HEIGHT_DP = 56
    const val MAIN_HEADER_HEIGHT_DP = 56
    const val INSPECTOR_WIDTH_DP = 400
    const val MINIMUM_INSPECTOR_WIDTH_DP = 360
    const val MINIMUM_CENTER_WIDTH_DP = 560
    const val CANVAS_HEADER_HEIGHT_DP = 64
    const val CANVAS_ACTION_HEIGHT_DP = 72
    const val CANVAS_BODY_MAX_WIDTH_DP = 960
    const val DIVIDER_WIDTH_DP = 1
}

enum class ShellRegion {
    GlobalTopBar,
    Sidebar,
    MainHeader,
    MainBody,
    Inspector,
    CanvasHeader,
    CanvasBody,
    CanvasActionBar,
}

enum class ScrollOwnership { None, InternalPane, CanvasBodyAccessibility }

enum class InspectorPlacement { Hidden, Beside, Overlay }

data class ShellLayoutRegion(
    val region: ShellRegion,
    val fixed: Boolean,
    val focusOrder: Int,
    val scrollOwnership: ScrollOwnership = ScrollOwnership.None,
)

val dashboardRegions =
    listOf(
        ShellLayoutRegion(ShellRegion.GlobalTopBar, fixed = true, focusOrder = 0),
        ShellLayoutRegion(ShellRegion.Sidebar, fixed = true, focusOrder = 1),
        ShellLayoutRegion(ShellRegion.MainHeader, fixed = true, focusOrder = 2),
        ShellLayoutRegion(
            ShellRegion.MainBody,
            fixed = false,
            focusOrder = 3,
            scrollOwnership = ScrollOwnership.InternalPane,
        ),
        ShellLayoutRegion(
            ShellRegion.Inspector,
            fixed = false,
            focusOrder = 4,
            scrollOwnership = ScrollOwnership.InternalPane,
        ),
    )

val canvasRegions =
    listOf(
        ShellLayoutRegion(ShellRegion.CanvasHeader, fixed = true, focusOrder = 0),
        ShellLayoutRegion(ShellRegion.CanvasBody, fixed = false, focusOrder = 1),
        ShellLayoutRegion(ShellRegion.CanvasActionBar, fixed = true, focusOrder = 2),
    )

fun inspectorPlacement(
    windowWidthDp: Int,
    inspectorRequested: Boolean,
): InspectorPlacement {
    if (!inspectorRequested) return InspectorPlacement.Hidden
    val besideThreshold =
        ShellDimensions.SIDEBAR_WIDTH_DP +
            ShellDimensions.MINIMUM_CENTER_WIDTH_DP +
            ShellDimensions.MINIMUM_INSPECTOR_WIDTH_DP
    return if (windowWidthDp >= besideThreshold) InspectorPlacement.Beside else InspectorPlacement.Overlay
}

fun canvasBodyScroll(textSize: TextSizePreference): ScrollOwnership =
    if (textSize == TextSizePreference.VeryLarge) {
        ScrollOwnership.CanvasBodyAccessibility
    } else {
        ScrollOwnership.None
    }
