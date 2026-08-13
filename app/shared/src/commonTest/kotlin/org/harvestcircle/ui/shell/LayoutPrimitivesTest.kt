package org.harvestcircle.ui.shell

import org.harvestcircle.appearance.TextSizePreference
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class LayoutPrimitivesTest {
    @Test
    fun dimensionsAndFocusGroupsMatchTheLockedLayout() {
        assertEquals(1280, ShellDimensions.PREFERRED_WINDOW_WIDTH_DP)
        assertEquals(1100, ShellDimensions.MINIMUM_WINDOW_WIDTH_DP)
        assertEquals(232, ShellDimensions.SIDEBAR_WIDTH_DP)
        assertEquals(960, ShellDimensions.CANVAS_BODY_MAX_WIDTH_DP)
        assertEquals(dashboardRegions.indices.toList(), dashboardRegions.map(ShellLayoutRegion::focusOrder))
        assertTrue(dashboardRegions.filter(ShellLayoutRegion::fixed).map(ShellLayoutRegion::region).contains(ShellRegion.Sidebar))
    }

    @Test
    fun scrollingIsOwnedByBoundedInternalRegions() {
        assertEquals(ScrollOwnership.InternalPane, dashboardRegions.single { it.region == ShellRegion.MainBody }.scrollOwnership)
        assertEquals(ScrollOwnership.None, canvasBodyScroll(TextSizePreference.Default))
        assertEquals(ScrollOwnership.CanvasBodyAccessibility, canvasBodyScroll(TextSizePreference.VeryLarge))
    }

    @Test
    fun inspectorMovesBesideOrOverlaysAtTheUsefulCenterBreakpoint() {
        assertEquals(InspectorPlacement.Beside, inspectorPlacement(1280, inspectorRequested = true))
        assertEquals(InspectorPlacement.Overlay, inspectorPlacement(1100, inspectorRequested = true))
        assertEquals(InspectorPlacement.Hidden, inspectorPlacement(1280, inspectorRequested = false))
    }
}
