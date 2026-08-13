package org.harvestcircle.designsystem.shell

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

@Composable
public fun HarvestCircleShellPage(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        modifier =
            modifier
                .fillMaxSize()
                .background(HarvestCircleShellPalette.pane)
                .padding(
                    horizontal = HarvestCircleShellMetrics.contentPageHorizontalInset,
                    vertical = HarvestCircleShellMetrics.contentPageVerticalInset,
                ),
        verticalArrangement = Arrangement.spacedBy(16.dp),
        content = content,
    )
}

@Composable
public fun HarvestCircleShellPanel(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    val colors = HarvestCircleShellPalette
    val shape =
        androidx.compose.foundation.shape
            .RoundedCornerShape(HarvestCircleShellMetrics.contentPanelRadius)
    Column(
        modifier =
            modifier
                .fillMaxWidth()
                .clip(shape)
                .background(colors.raised)
                .border(BorderStroke(1.dp, colors.border), shape)
                .padding(HarvestCircleShellMetrics.contentPanelInset),
        verticalArrangement = Arrangement.spacedBy(10.dp),
        content = content,
    )
}

@Composable
public fun HarvestCircleShellEmptyState(
    title: String,
    body: List<String>,
    modifier: Modifier = Modifier,
    context: String? = null,
    showIllustration: Boolean = true,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Box(
        modifier = modifier.fillMaxSize().background(HarvestCircleShellPalette.pane),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier =
                Modifier
                    .width(HarvestCircleShellMetrics.emptyStateWidth)
                    .offset(y = HarvestCircleShellMetrics.emptyStateVerticalOffset),
            horizontalAlignment = Alignment.Start,
        ) {
            if (showIllustration) {
                Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                    HarvestCircleStackedDocumentsIllustration()
                }
                Spacer(Modifier.height(HarvestCircleShellMetrics.emptyStateIllustrationToTitleGap))
            }
            context?.let {
                HarvestCircleShellText(
                    it,
                    role = HarvestCircleShellTextRole.Small,
                    color = HarvestCircleShellPalette.contentSecondary,
                    maxLines = 1,
                )
                Spacer(Modifier.height(HarvestCircleShellMetrics.emptyStateTitleToBodyGap))
            }
            HarvestCircleShellText(
                title,
                Modifier.semantics { heading() },
                HarvestCircleShellTextRole.SectionTitle,
            )
            body.forEachIndexed { index, paragraph ->
                Spacer(
                    Modifier.height(
                        if (index == 0) {
                            HarvestCircleShellMetrics.emptyStateTitleToBodyGap
                        } else {
                            HarvestCircleShellMetrics.emptyStateParagraphGap
                        },
                    ),
                )
                HarvestCircleShellText(paragraph, color = HarvestCircleShellPalette.contentSecondary)
            }
            Row(
                modifier = Modifier.padding(top = HarvestCircleShellMetrics.emptyStateBodyToActionsGap),
                horizontalArrangement = Arrangement.spacedBy(HarvestCircleShellMetrics.emptyStateActionGap),
                verticalAlignment = Alignment.CenterVertically,
                content = actions,
            )
        }
    }
}

@Composable
private fun HarvestCircleStackedDocumentsIllustration() {
    val lineColor = HarvestCircleShellPalette.contentMuted
    Canvas(
        modifier =
            Modifier.size(
                HarvestCircleShellMetrics.emptyStateIllustrationWidth,
                HarvestCircleShellMetrics.emptyStateIllustrationHeight,
            ),
    ) {
        val strokeWidth = HarvestCircleShellMetrics.emptyStateIllustrationStrokeWidth.toPx()
        val stroke = Stroke(strokeWidth, cap = StrokeCap.Round, join = StrokeJoin.Round)
        val scaleX = size.width / 56f
        val scaleY = size.height / 64f

        fun x(value: Float): Float = value * scaleX

        fun y(value: Float): Float = value * scaleY

        fun sheet(
            top: Float,
            inset: Float,
        ): Path =
            Path().apply {
                moveTo(x(inset), y(top + 8f))
                lineTo(x(44f), y(top))
                lineTo(x(56f - inset), y(top + 8f))
                lineTo(x(56f - inset), y(top + 22f))
                lineTo(x(28f), y(top + 31f))
                lineTo(x(inset), y(top + 22f))
                close()
            }
        drawPath(sheet(27f, 10f), lineColor, style = stroke)
        drawPath(sheet(22f, 9f), lineColor, style = stroke)
        drawPath(sheet(17f, 8f), lineColor, style = stroke)
        val topDocument =
            Path().apply {
                moveTo(x(15f), y(13f))
                lineTo(x(38f), y(5f))
                lineTo(x(49f), y(9f))
                lineTo(x(40f), y(27f))
                lineTo(x(17f), y(33f))
                lineTo(x(9f), y(29f))
                close()
            }
        drawPath(topDocument, lineColor, style = stroke)
        drawLine(lineColor, Offset(x(17f), y(33f)), Offset(x(40f), y(27f)), strokeWidth, StrokeCap.Round)
    }
}
