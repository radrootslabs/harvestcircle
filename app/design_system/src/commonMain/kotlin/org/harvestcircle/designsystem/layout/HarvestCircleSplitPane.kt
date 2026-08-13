package org.harvestcircle.designsystem.layout

import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.ExperimentalHarvestCircleUiApi
import org.harvestcircle.designsystem.component.utility.HarvestCircleVerticalDivider
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Hoisted state for [HarvestCircleHorizontalSplitPane]. */
@ExperimentalHarvestCircleUiApi
@Stable
public class HarvestCircleSplitPaneState internal constructor(
    initialFraction: Float,
    public val minimumFraction: Float,
    public val maximumFraction: Float,
) {
    private var mutableFraction: Float by
        mutableFloatStateOf(initialFraction.coerceIn(minimumFraction, maximumFraction))

    public val fraction: Float
        get() = mutableFraction

    public fun setFraction(value: Float) {
        mutableFraction = value.coerceIn(minimumFraction, maximumFraction)
    }

    internal fun offsetBy(delta: Float) {
        setFraction(fraction + delta)
    }

    internal companion object {
        fun saver(
            minimumFraction: Float,
            maximumFraction: Float,
        ): Saver<HarvestCircleSplitPaneState, Float> =
            Saver(
                save = { it.fraction },
                restore = {
                    HarvestCircleSplitPaneState(
                        initialFraction = it,
                        minimumFraction = minimumFraction,
                        maximumFraction = maximumFraction,
                    )
                },
            )
    }
}

/** Creates saveable split-pane state. */
@ExperimentalHarvestCircleUiApi
@Composable
public fun rememberHarvestCircleSplitPaneState(
    initialFraction: Float = 0.32f,
    minimumFraction: Float = 0.18f,
    maximumFraction: Float = 0.82f,
): HarvestCircleSplitPaneState {
    require(minimumFraction in 0f..1f)
    require(maximumFraction in 0f..1f)
    require(minimumFraction < maximumFraction)

    return rememberSaveable(
        minimumFraction,
        maximumFraction,
        saver = HarvestCircleSplitPaneState.saver(minimumFraction, maximumFraction),
    ) {
        HarvestCircleSplitPaneState(
            initialFraction = initialFraction,
            minimumFraction = minimumFraction,
            maximumFraction = maximumFraction,
        )
    }
}

/**
 * Accessible horizontal split pane supporting pointer drag, arrow keys, and semantic progress
 * adjustment.
 */
@ExperimentalHarvestCircleUiApi
@Composable
public fun HarvestCircleHorizontalSplitPane(
    dividerLabel: String,
    modifier: Modifier = Modifier,
    state: HarvestCircleSplitPaneState = rememberHarvestCircleSplitPaneState(),
    first: @Composable BoxScope.() -> Unit,
    second: @Composable BoxScope.() -> Unit,
) {
    BoxWithConstraints(modifier = modifier) {
        val density = LocalDensity.current
        val availableWidthPx = with(density) { maxWidth.toPx() }.coerceAtLeast(1f)
        var dividerFocused by remember { mutableStateOf(false) }

        Row(Modifier.fillMaxSize()) {
            Box(
                modifier =
                    Modifier
                        .weight(state.fraction)
                        .fillMaxHeight(),
                content = first,
            )

            Box(
                modifier =
                    Modifier
                        .width(8.dp)
                        .fillMaxHeight()
                        .background(
                            if (dividerFocused) {
                                HarvestCircleTheme.foundation.colors.surface.selected
                            } else {
                                HarvestCircleTheme.foundation.colors.surface.base
                            },
                        ).semantics {
                            contentDescription = dividerLabel
                            progressBarRangeInfo =
                                ProgressBarRangeInfo(
                                    current = state.fraction,
                                    range = state.minimumFraction..state.maximumFraction,
                                    steps = 0,
                                )
                            setProgress { requested ->
                                state.setFraction(requested)
                                true
                            }
                        }.onFocusChanged {
                            dividerFocused = it.isFocused
                        }.focusable()
                        .onKeyEvent { event ->
                            if (event.type != KeyEventType.KeyDown) {
                                return@onKeyEvent false
                            }

                            when (event.key) {
                                Key.DirectionLeft -> {
                                    state.offsetBy(-0.04f)
                                    true
                                }

                                Key.DirectionRight -> {
                                    state.offsetBy(0.04f)
                                    true
                                }

                                else -> false
                            }
                        }.pointerInput(availableWidthPx) {
                            detectDragGestures { change, dragAmount ->
                                change.consume()
                                state.offsetBy(dragAmount.x / availableWidthPx)
                            }
                        },
                contentAlignment = Alignment.Center,
            ) {
                HarvestCircleVerticalDivider(
                    modifier =
                        Modifier
                            .width(HarvestCircleTheme.shell.dimensions.dividerWidth)
                            .fillMaxHeight(),
                    strong = dividerFocused,
                )
            }

            Box(
                modifier =
                    Modifier
                        .weight(1f - state.fraction)
                        .fillMaxHeight(),
                content = second,
            )
        }
    }
}
