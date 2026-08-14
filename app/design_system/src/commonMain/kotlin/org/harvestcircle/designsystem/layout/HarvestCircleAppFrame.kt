package org.harvestcircle.designsystem.layout

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.absolutePadding
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.shell.HarvestCircleShellPalette
import org.harvestcircle.designsystem.theme.HarvestCircleFrameMetrics
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

public enum class HarvestCircleFrameLayoutClass {
    Compact,
    Medium,
    Expanded,
}

public enum class HarvestCirclePaneWidth {
    Narrow,
    Standard,
    Wide,
    Utility,
    Inspector,
}

@Immutable
public data class HarvestCircleFrameGeometry(
    public val layoutClass: HarvestCircleFrameLayoutClass,
    public val sidebarWidth: Dp,
    public val showSecondaryPane: Boolean,
    public val showUtilityPane: Boolean,
    public val sidebarChromeClearance: HarvestCircleWindowChromeClearance,
    public val topBarChromeClearance: HarvestCircleWindowChromeClearance,
    public val sidebarTopBandFullyExcluded: Boolean,
)

public class HarvestCirclePaneSlot(
    public val width: HarvestCirclePaneWidth,
    public val header: @Composable () -> Unit,
    public val content: @Composable BoxScope.() -> Unit,
)

internal fun HarvestCirclePaneWidth.resolve(metrics: HarvestCircleFrameMetrics): Dp =
    when (this) {
        HarvestCirclePaneWidth.Narrow -> metrics.narrowPaneWidth
        HarvestCirclePaneWidth.Standard -> metrics.standardPaneWidth
        HarvestCirclePaneWidth.Wide -> metrics.widePaneWidth
        HarvestCirclePaneWidth.Utility -> metrics.utilityPaneWidth
        HarvestCirclePaneWidth.Inspector -> metrics.inspectorPaneWidth
    }

public fun resolveHarvestCircleFrameGeometry(
    width: Dp,
    sidebarCollapsed: Boolean,
    metrics: HarvestCircleFrameMetrics,
    secondaryWidth: HarvestCirclePaneWidth? = null,
    utilityWidth: HarvestCirclePaneWidth? = null,
    windowChromeExclusion: HarvestCircleWindowChromeExclusion = HarvestCircleWindowChromeExclusion.None,
): HarvestCircleFrameGeometry {
    val layoutClass =
        when {
            width < metrics.compactBreakpoint -> HarvestCircleFrameLayoutClass.Compact
            width < metrics.expandedBreakpoint -> HarvestCircleFrameLayoutClass.Medium
            else -> HarvestCircleFrameLayoutClass.Expanded
        }
    val sidebarWidth =
        if (sidebarCollapsed || layoutClass == HarvestCircleFrameLayoutClass.Compact) {
            metrics.collapsedSidebarWidth
        } else {
            metrics.sidebarWidth
        }

    fun fits(
        includeSecondary: Boolean,
        includeUtility: Boolean,
    ): Boolean {
        val paneDividerCount = (if (includeSecondary) 1 else 0) + (if (includeUtility) 1 else 0)
        val dividerWidth = metrics.structuralDividerWidth
        val requiredWidth =
            sidebarWidth +
                dividerWidth +
                metrics.mainPaneMinimumWidth +
                (if (includeSecondary) secondaryWidth?.resolve(metrics) ?: 0.dp else 0.dp) +
                (if (includeUtility) utilityWidth?.resolve(metrics) ?: 0.dp else 0.dp) +
                dividerWidth * paneDividerCount
        return requiredWidth <= width
    }

    val secondaryRequested = secondaryWidth != null
    val utilityRequested = utilityWidth != null
    val (showSecondary, showUtility) =
        when (layoutClass) {
            HarvestCircleFrameLayoutClass.Compact -> false to false
            HarvestCircleFrameLayoutClass.Medium -> {
                val secondary = secondaryRequested && fits(true, false)
                secondary to (!secondary && utilityRequested && fits(false, true))
            }
            HarvestCircleFrameLayoutClass.Expanded -> {
                val secondary = secondaryRequested && fits(true, false)
                secondary to (utilityRequested && fits(secondary, true))
            }
        }
    val sidebarRegionWidth = minOf(sidebarWidth, width)
    val topBarRegionLeft = minOf(width, sidebarWidth + metrics.structuralDividerWidth)
    val sidebarChromeClearance =
        resolveHarvestCircleWindowChromeClearance(
            exclusion = windowChromeExclusion,
            windowWidth = width,
            regionLeft = 0.dp,
            regionWidth = sidebarRegionWidth,
            minimumTopBandHeight = metrics.topBarHeight,
        )
    val topBarChromeClearance =
        resolveHarvestCircleWindowChromeClearance(
            exclusion = windowChromeExclusion,
            windowWidth = width,
            regionLeft = topBarRegionLeft,
            regionWidth = width - topBarRegionLeft,
            minimumTopBandHeight = metrics.topBarHeight,
        )

    return HarvestCircleFrameGeometry(
        layoutClass = layoutClass,
        sidebarWidth = sidebarWidth,
        showSecondaryPane = showSecondary,
        showUtilityPane = showUtility,
        sidebarChromeClearance = sidebarChromeClearance,
        topBarChromeClearance = topBarChromeClearance,
        sidebarTopBandFullyExcluded =
            sidebarRegionWidth > 0.dp &&
                sidebarChromeClearance.left + sidebarChromeClearance.right >= sidebarRegionWidth,
    )
}

@Composable
public fun HarvestCircleAppFrame(
    sidebarCollapsed: Boolean,
    sidebar: @Composable (HarvestCircleFrameGeometry) -> Unit,
    topBar: @Composable (HarvestCircleFrameGeometry) -> Unit,
    mainHeader: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    secondaryPane: HarvestCirclePaneSlot? = null,
    utilityPane: HarvestCirclePaneSlot? = null,
    mainContent: @Composable BoxScope.() -> Unit,
) {
    val frameMetrics = HarvestCircleTheme.shell.frame

    val shellColors = HarvestCircleShellPalette
    HarvestCircleSurface(
        modifier = modifier.fillMaxSize(),
        role = HarvestCircleSurfaceRole.Canvas,
        color = shellColors.viewportCanvas,
    ) {
        BoxWithConstraints(
            modifier =
                Modifier
                    .fillMaxSize()
                    .testTag("harvestcircle-frame"),
        ) {
            val geometry =
                resolveHarvestCircleFrameGeometry(
                    width = maxWidth,
                    sidebarCollapsed = sidebarCollapsed,
                    metrics = frameMetrics,
                    secondaryWidth = secondaryPane?.width,
                    utilityWidth = utilityPane?.width,
                    windowChromeExclusion = HarvestCircleWindowChrome.exclusion,
                )

            Row(Modifier.fillMaxSize()) {
                Box(
                    modifier =
                        Modifier
                            .width(geometry.sidebarWidth)
                            .fillMaxHeight()
                            .background(shellColors.sidebar)
                            .testTag("harvestcircle-sidebar"),
                ) {
                    sidebar(geometry)
                }
                HarvestCircleStructuralDivider(vertical = true)
                Column(
                    modifier =
                        Modifier
                            .weight(1f)
                            .fillMaxHeight(),
                ) {
                    Box(
                        modifier =
                            Modifier
                                .fillMaxWidth()
                                .height(geometry.topBarChromeClearance.topBandHeight)
                                .background(shellColors.applicationFrame)
                                .testTag("harvestcircle-top-bar"),
                    ) {
                        Box(
                            Modifier
                                .fillMaxSize()
                                .absolutePadding(
                                    left = geometry.topBarChromeClearance.left,
                                    right = geometry.topBarChromeClearance.right,
                                ).testTag("harvestcircle-top-bar-chrome-content"),
                        ) {
                            topBar(geometry)
                        }
                    }
                    HarvestCircleStructuralDivider(vertical = false)
                    Row(
                        modifier =
                            Modifier
                                .weight(1f)
                                .fillMaxSize(),
                    ) {
                        if (geometry.showSecondaryPane && secondaryPane != null) {
                            HarvestCircleFramePane(
                                modifier =
                                    Modifier
                                        .width(secondaryPane.width.resolve(frameMetrics))
                                        .fillMaxHeight()
                                        .testTag("harvestcircle-secondary-pane"),
                                slot = secondaryPane,
                            )
                            HarvestCircleStructuralDivider(vertical = true)
                        }
                        HarvestCircleFramePane(
                            modifier =
                                Modifier
                                    .weight(1f)
                                    .fillMaxHeight()
                                    .widthIn(min = frameMetrics.mainPaneMinimumWidth)
                                    .testTag("harvestcircle-main-pane"),
                            header = mainHeader,
                            content = mainContent,
                        )
                        if (geometry.showUtilityPane && utilityPane != null) {
                            HarvestCircleStructuralDivider(vertical = true)
                            HarvestCircleFramePane(
                                modifier =
                                    Modifier
                                        .width(utilityPane.width.resolve(frameMetrics))
                                        .fillMaxHeight()
                                        .testTag("harvestcircle-utility-pane"),
                                slot = utilityPane,
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun HarvestCircleFramePane(
    slot: HarvestCirclePaneSlot,
    modifier: Modifier = Modifier,
): Unit =
    HarvestCircleFramePane(
        header = slot.header,
        content = slot.content,
        modifier = modifier,
    )

@Composable
private fun HarvestCircleFramePane(
    header: @Composable () -> Unit,
    content: @Composable BoxScope.() -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.background(HarvestCircleShellPalette.pane)) {
        Box(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .height(HarvestCircleTheme.shell.frame.localHeaderHeight),
        ) {
            header()
        }
        HarvestCircleStructuralDivider(vertical = false)
        Box(
            modifier =
                Modifier
                    .weight(1f)
                    .fillMaxSize(),
            content = content,
        )
    }
}

@Composable
public fun HarvestCircleStructuralDivider(
    vertical: Boolean,
    modifier: Modifier = Modifier,
) {
    val dividerModifier =
        if (vertical) {
            modifier
                .width(HarvestCircleTheme.shell.frame.structuralDividerWidth)
                .fillMaxHeight()
        } else {
            modifier
                .fillMaxWidth()
                .height(HarvestCircleTheme.shell.frame.structuralDividerWidth)
        }
    Box(dividerModifier.background(HarvestCircleShellPalette.divider))
}
