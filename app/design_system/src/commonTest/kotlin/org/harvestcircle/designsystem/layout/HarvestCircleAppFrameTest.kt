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
}
