package org.harvestcircle.designsystem.shell

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals

class HarvestCircleShellVisualsTest {
    @Test
    fun palettePinsApprovedStudioValues() {
        val light = harvestCircleShellColors(false)
        val dark = harvestCircleShellColors(true)
        assertEquals(Color(0xFF155239), light.accent)
        assertEquals(Color(0xFFF6F6F4), light.sidebar)
        assertEquals(Color(0xFFECECEA), light.navigationSelected)
        assertEquals(Color(0xFF141517), light.contentPrimary)
        assertEquals(Color(0xFF155239), dark.accent)
        assertNotEquals(light.applicationFrame, dark.applicationFrame)
    }

    @Test
    fun geometryPinsApprovedStudioGrid() {
        assertEquals(48.dp, HarvestCircleShellMetrics.topBarHeight)
        assertEquals(40.dp, HarvestCircleShellMetrics.localHeaderHeight)
        assertEquals(232.dp, HarvestCircleShellMetrics.sidebarWidth)
        assertEquals(72.dp, HarvestCircleShellMetrics.collapsedSidebarWidth)
        assertEquals(344.dp, HarvestCircleShellMetrics.emptyStateWidth)
        assertEquals(56.dp, HarvestCircleShellMetrics.emptyStateIllustrationWidth)
        assertEquals(64.dp, HarvestCircleShellMetrics.emptyStateIllustrationHeight)
        assertEquals(976.dp, HarvestCircleShellMetrics.compactBreakpoint)
        assertEquals(1272.dp, HarvestCircleShellMetrics.expandedBreakpoint)
    }
}
