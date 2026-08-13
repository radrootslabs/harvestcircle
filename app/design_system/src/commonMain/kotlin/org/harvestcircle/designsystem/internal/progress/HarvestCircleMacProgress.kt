package org.harvestcircle.designsystem.internal.progress

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import kotlin.math.floor
import kotlin.math.max

@Composable
internal fun HarvestCircleMacSpinner(
    modifier: Modifier = Modifier,
    size: Dp = HarvestCircleTheme.shell.dimensions.iconMedium,
    color: Color = HarvestCircleTheme.foundation.colors.content.secondary,
) {
    val duration = if (HarvestCircleTheme.component.motion.standardMillis == 0) 0 else 900
    val phase =
        if (duration == 0) {
            0f
        } else {
            val transition = rememberInfiniteTransition(label = "HarvestCircleMacSpinner")
            transition
                .animateFloat(
                    initialValue = 0f,
                    targetValue = 12f,
                    animationSpec =
                        infiniteRepeatable(
                            animation = tween(durationMillis = duration, easing = LinearEasing),
                            repeatMode = RepeatMode.Restart,
                        ),
                    label = "HarvestCircleMacSpinnerPhase",
                ).value
        }

    Canvas(modifier = modifier.size(size)) {
        val radius = this.size.minDimension / 2f
        val center = Offset(this.size.width / 2f, this.size.height / 2f)
        val inner = radius * 0.43f
        val outer = radius * 0.88f
        val stroke = max(1f, radius * 0.17f)
        val head = floor(phase).toInt().mod(12)

        repeat(12) { index ->
            val distance = (index - head + 12).mod(12)
            val alpha = 0.16f + ((11 - distance) / 11f) * 0.84f
            rotate(
                degrees = index * 30f,
                pivot = center,
            ) {
                drawLine(
                    color = color.copy(alpha = color.alpha * alpha),
                    start = Offset(center.x, center.y - inner),
                    end = Offset(center.x, center.y - outer),
                    strokeWidth = stroke,
                    cap = StrokeCap.Round,
                )
            }
        }
    }
}

@Composable
internal fun HarvestCircleMacIndeterminateBar(
    modifier: Modifier = Modifier,
    color: Color = HarvestCircleTheme.foundation.colors.action.primary.rest,
) {
    val duration = if (HarvestCircleTheme.component.motion.standardMillis == 0) 0 else 1100
    val phase =
        if (duration == 0) {
            0.45f
        } else {
            val transition = rememberInfiniteTransition(label = "HarvestCircleMacProgressBar")
            transition
                .animateFloat(
                    initialValue = 0f,
                    targetValue = 1f,
                    animationSpec =
                        infiniteRepeatable(
                            animation = tween(durationMillis = duration, easing = LinearEasing),
                            repeatMode = RepeatMode.Restart,
                        ),
                    label = "HarvestCircleMacProgressBarPhase",
                ).value
        }

    val track = HarvestCircleTheme.foundation.colors.surface.sunken
    Canvas(modifier = modifier.height(4.dp)) {
        val radius = size.height / 2f
        drawRoundRect(
            color = track,
            size = size,
            cornerRadius = CornerRadius(radius, radius),
        )

        val segmentWidth = size.width * 0.32f
        val travel = size.width + segmentWidth
        val x = phase * travel - segmentWidth
        drawRoundRect(
            color = color,
            topLeft = Offset(x, 0f),
            size = Size(segmentWidth, size.height),
            cornerRadius = CornerRadius(radius, radius),
        )
    }
}
