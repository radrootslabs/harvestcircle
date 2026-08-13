package org.harvestcircle.designsystem.layout

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
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
    return when (layoutClass) {
        HarvestCircleFrameLayoutClass.Compact ->
            HarvestCircleFrameGeometry(layoutClass, sidebarWidth, false, false)

        HarvestCircleFrameLayoutClass.Medium -> {
            val showSecondary = secondaryRequested && fits(true, false)
            val showUtility = !showSecondary && utilityRequested && fits(false, true)
            HarvestCircleFrameGeometry(layoutClass, sidebarWidth, showSecondary, showUtility)
        }

        HarvestCircleFrameLayoutClass.Expanded -> {
            val showSecondary = secondaryRequested && fits(true, false)
            val showUtility = utilityRequested && fits(showSecondary, true)
            HarvestCircleFrameGeometry(layoutClass, sidebarWidth, showSecondary, showUtility)
        }
    }
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
                                .height(frameMetrics.topBarHeight)
                                .testTag("harvestcircle-top-bar"),
                    ) {
                        topBar(geometry)
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
