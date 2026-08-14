package org.harvestcircle.designsystem.layout

import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class HarvestCircleWindowChromeTest {
    @Test
    fun noExclusionKeepsTheDesignBandAndAddsNoClearance() {
        val clearance = resolve(regionLeft = 0.dp, regionWidth = 1280.dp)

        assertEquals(48.dp, clearance.topBandHeight)
        assertEquals(0.dp, clearance.left)
        assertEquals(0.dp, clearance.right)
    }

    @Test
    fun leftExclusionClearsOnlyTheIntersectingPortionOfEachRegion() {
        val exclusion =
            HarvestCircleWindowChromeExclusion(
                HarvestCircleWindowChromeEdge.Left,
                width = 112.dp,
                height = 40.dp,
            )

        assertEquals(112.dp, resolve(exclusion, regionLeft = 0.dp, regionWidth = 1280.dp).left)
        assertEquals(40.dp, resolve(exclusion, regionLeft = 72.dp, regionWidth = 1208.dp).left)
        assertEquals(0.dp, resolve(exclusion, regionLeft = 232.dp, regionWidth = 1048.dp).left)
    }

    @Test
    fun rightExclusionResolvesAgainstPhysicalWindowCoordinates() {
        val exclusion =
            HarvestCircleWindowChromeExclusion(
                HarvestCircleWindowChromeEdge.Right,
                width = 96.dp,
                height = 40.dp,
            )

        val clearance = resolve(exclusion, regionLeft = 72.dp, regionWidth = 1208.dp)

        assertEquals(0.dp, clearance.left)
        assertEquals(96.dp, clearance.right)
    }

    @Test
    fun tallerHostChromeExpandsTheTopBand() {
        val exclusion =
            HarvestCircleWindowChromeExclusion(
                HarvestCircleWindowChromeEdge.Left,
                width = 112.dp,
                height = 56.dp,
            )

        assertEquals(56.dp, resolve(exclusion, regionLeft = 0.dp, regionWidth = 1280.dp).topBandHeight)
    }

    @Test
    fun exclusionAndClearanceClampToNarrowWindowsAndRegions() {
        val exclusion =
            HarvestCircleWindowChromeExclusion(
                HarvestCircleWindowChromeEdge.Left,
                width = 112.dp,
                height = 40.dp,
            )

        assertEquals(
            24.dp,
            resolve(
                exclusion = exclusion,
                windowWidth = 80.dp,
                regionLeft = 56.dp,
                regionWidth = 24.dp,
            ).left,
        )
    }

    @Test
    fun invalidGeometryFailsClosed() {
        assertFailsWith<IllegalArgumentException> {
            HarvestCircleWindowChromeExclusion(
                HarvestCircleWindowChromeEdge.Left,
                width = (-1).dp,
                height = 40.dp,
            )
        }
        assertFailsWith<IllegalArgumentException> {
            resolve(regionLeft = 1200.dp, regionWidth = 100.dp)
        }
    }

    private fun resolve(
        exclusion: HarvestCircleWindowChromeExclusion = HarvestCircleWindowChromeExclusion.None,
        windowWidth: Dp = 1280.dp,
        regionLeft: Dp,
        regionWidth: Dp,
    ): HarvestCircleWindowChromeClearance =
        resolveHarvestCircleWindowChromeClearance(
            exclusion = exclusion,
            windowWidth = windowWidth,
            regionLeft = regionLeft,
            regionWidth = regionWidth,
            minimumTopBandHeight = 48.dp,
        )
}
