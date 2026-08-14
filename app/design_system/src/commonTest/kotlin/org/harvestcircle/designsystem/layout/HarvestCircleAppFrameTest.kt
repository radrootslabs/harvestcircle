package org.harvestcircle.designsystem.layout

import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.theme.HarvestCircleDefaultFrameMetrics
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class HarvestCircleAppFrameTest {
    @Test
    fun compactFrameCollapsesSidebarAndHidesOptionalPanes() {
        val geometry =
            resolveHarvestCircleFrameGeometry(
                width = 900.dp,
                sidebarCollapsed = false,
                metrics = HarvestCircleDefaultFrameMetrics,
                secondaryWidth = HarvestCirclePaneWidth.Standard,
                utilityWidth = HarvestCirclePaneWidth.Utility,
            )

        assertEquals(HarvestCircleFrameLayoutClass.Compact, geometry.layoutClass)
        assertEquals(HarvestCircleDefaultFrameMetrics.collapsedSidebarWidth, geometry.sidebarWidth)
        assertFalse(geometry.showSecondaryPane)
        assertFalse(geometry.showUtilityPane)
    }

    @Test
    fun expandedFrameAdmitsBothPanesOnlyWhenTheyFit() {
        val geometry =
            resolveHarvestCircleFrameGeometry(
                width = 1500.dp,
                sidebarCollapsed = false,
                metrics = HarvestCircleDefaultFrameMetrics,
                secondaryWidth = HarvestCirclePaneWidth.Standard,
                utilityWidth = HarvestCirclePaneWidth.Utility,
            )

        assertEquals(HarvestCircleFrameLayoutClass.Expanded, geometry.layoutClass)
        assertTrue(geometry.showSecondaryPane)
        assertTrue(geometry.showUtilityPane)
    }

    @Test
    fun expandedSidebarAbsorbsTheMacOsChromeExclusion() {
        val geometry =
            resolveHarvestCircleFrameGeometry(
                width = 1280.dp,
                sidebarCollapsed = false,
                metrics = HarvestCircleDefaultFrameMetrics,
                windowChromeExclusion = macOsExclusion(),
            )

        assertEquals(112.dp, geometry.sidebarChromeClearance.left)
        assertEquals(0.dp, geometry.topBarChromeClearance.left)
        assertFalse(geometry.sidebarTopBandFullyExcluded)
    }

    @Test
    fun collapsedSidebarMovesResidualMacOsClearanceIntoTheTopBar() {
        val geometry =
            resolveHarvestCircleFrameGeometry(
                width = 1280.dp,
                sidebarCollapsed = true,
                metrics = HarvestCircleDefaultFrameMetrics,
                windowChromeExclusion = macOsExclusion(),
            )

        assertEquals(72.dp, geometry.sidebarChromeClearance.left)
        assertEquals(39.dp, geometry.topBarChromeClearance.left)
        assertTrue(geometry.sidebarTopBandFullyExcluded)
    }

    private fun macOsExclusion(): HarvestCircleWindowChromeExclusion =
        HarvestCircleWindowChromeExclusion(
            edge = HarvestCircleWindowChromeEdge.Left,
            width = 112.dp,
            height = 40.dp,
        )
}
