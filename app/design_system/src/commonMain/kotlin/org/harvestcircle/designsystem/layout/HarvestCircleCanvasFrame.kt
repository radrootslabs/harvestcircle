package org.harvestcircle.designsystem.layout

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import org.harvestcircle.designsystem.shell.HarvestCircleShellMetrics
import org.harvestcircle.designsystem.shell.HarvestCircleShellPalette

/** Centered bootstrap and lifecycle frame using the approved Studio-derived shell chrome. */
@Composable
public fun HarvestCircleCanvasFrame(
    header: @Composable () -> Unit,
    body: @Composable () -> Unit,
    actionBar: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    bodyModifier: Modifier = Modifier,
    navigation: @Composable () -> Unit = {},
    step: @Composable () -> Unit = {},
    bodyScrollable: Boolean = false,
) {
    val colors = HarvestCircleShellPalette
    Box(modifier.fillMaxSize().background(colors.viewportCanvas), contentAlignment = Alignment.Center) {
        Column(Modifier.fillMaxSize().background(colors.pane)) {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .height(HarvestCircleShellMetrics.topBarHeight)
                        .background(colors.applicationFrame)
                        .padding(horizontal = HarvestCircleShellMetrics.contentPageHorizontalInset),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box { navigation() }
                Box(Modifier.weight(1f)) { header() }
                Box { step() }
            }
            HarvestCircleStructuralDivider(vertical = false)
            val scrollModifier = if (bodyScrollable) Modifier.verticalScroll(rememberScrollState()) else Modifier
            Box(
                modifier =
                    bodyModifier
                        .weight(1f)
                        .fillMaxWidth()
                        .background(colors.pane)
                        .then(scrollModifier),
                contentAlignment = Alignment.TopCenter,
            ) {
                Box(
                    Modifier
                        .widthIn(max = HarvestCircleShellMetrics.canvasContentMaxWidth)
                        .fillMaxWidth()
                        .padding(HarvestCircleShellMetrics.contentPageHorizontalInset),
                ) {
                    body()
                }
            }
            HarvestCircleStructuralDivider(vertical = false)
            Box(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .height(HarvestCircleShellMetrics.canvasActionBarHeight)
                        .background(colors.applicationFrame)
                        .padding(horizontal = HarvestCircleShellMetrics.contentPageHorizontalInset),
                contentAlignment = Alignment.CenterEnd,
            ) {
                actionBar()
            }
        }
    }
}
