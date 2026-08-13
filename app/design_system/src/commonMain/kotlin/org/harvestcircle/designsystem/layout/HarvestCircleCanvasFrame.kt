package org.harvestcircle.designsystem.layout

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
import org.harvestcircle.designsystem.component.utility.HarvestCircleHorizontalDivider
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Centered bootstrap and lifecycle frame with fixed chrome and one explicit scroll owner. */
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
    val frame = HarvestCircleTheme.shell.frame
    HarvestCircleSurface(modifier = modifier.fillMaxSize(), role = HarvestCircleSurfaceRole.Canvas) {
        Column(Modifier.fillMaxSize()) {
            HarvestCircleSurface(
                modifier = Modifier.fillMaxWidth().height(frame.canvasHeaderHeight),
                role = HarvestCircleSurfaceRole.Raised,
            ) {
                Row(
                    modifier = Modifier.fillMaxSize().padding(horizontal = HarvestCircleTheme.shell.layout.pageInset),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box { navigation() }
                    Box(Modifier.weight(1f)) { header() }
                    Box { step() }
                }
            }
            HarvestCircleHorizontalDivider()
            val scrollModifier =
                if (bodyScrollable) {
                    Modifier.verticalScroll(rememberScrollState())
                } else {
                    Modifier
                }
            Box(
                modifier = bodyModifier.weight(1f).fillMaxWidth().then(scrollModifier),
                contentAlignment = Alignment.TopCenter,
            ) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .widthIn(max = frame.canvasContentMaxWidth)
                        .padding(HarvestCircleTheme.shell.layout.pageInset),
                ) {
                    body()
                }
            }
            HarvestCircleHorizontalDivider()
            HarvestCircleSurface(
                modifier = Modifier.fillMaxWidth().height(frame.canvasActionBarHeight),
                role = HarvestCircleSurfaceRole.Raised,
            ) {
                Box(
                    modifier = Modifier.fillMaxSize().padding(horizontal = HarvestCircleTheme.shell.layout.pageInset),
                    contentAlignment = Alignment.CenterEnd,
                ) {
                    actionBar()
                }
            }
        }
    }
}
