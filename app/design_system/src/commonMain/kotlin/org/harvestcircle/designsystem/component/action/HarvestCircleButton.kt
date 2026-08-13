package org.harvestcircle.designsystem.component.action

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.component.HarvestCircleButtonVariant
import org.harvestcircle.designsystem.component.HarvestCircleControlSize
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.generated.resources.Res
import org.harvestcircle.designsystem.generated.resources.harvestcircle_loading
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacBezel
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacFocusFrame
import org.harvestcircle.designsystem.internal.chrome.harvestCircleMacControlBrush
import org.harvestcircle.designsystem.internal.chrome.harvestCircleMacFilledBorder
import org.harvestcircle.designsystem.internal.interaction.harvestCircleHoverable
import org.harvestcircle.designsystem.internal.interaction.harvestCircleInteractions
import org.harvestcircle.designsystem.internal.interaction.rememberHarvestCircleInteractionSources
import org.harvestcircle.designsystem.internal.progress.HarvestCircleMacSpinner
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.harvestcircle.designsystem.theme.color.HarvestCircleActionStateColors
import org.harvestcircle.designsystem.theme.color.HarvestCircleMacPrimarySubmitColors
import org.jetbrains.compose.resources.stringResource

/**
 * Pixel-sampled colors from the supplied canonical macOS default-action reference.
 *
 * These are component-level implementation tokens, not general product colors. The surrounding
 * blue contour is the native active-state halo shown in the reference; it appears only while the
 * button is pressed and is intentionally distinct from the optional keyboard [HarvestCircleFocusRing].
 */
private object HarvestCircleMacPrimarySubmitMetrics {
    val RingWidth = 3.dp
}

private data class HarvestCircleButtonMetrics(
    val visualHeight: Dp,
    val minimumVisualWidth: Dp,
    val horizontalPadding: Dp,
    val indicatorSize: Dp,
)

@Composable
private fun buttonMetrics(size: HarvestCircleControlSize): HarvestCircleButtonMetrics =
    when (size) {
        HarvestCircleControlSize.Small ->
            HarvestCircleButtonMetrics(
                visualHeight = HarvestCircleTheme.shell.dimensions.controlSmall,
                minimumVisualWidth = 48.dp,
                horizontalPadding = HarvestCircleTheme.foundation.spacing.md,
                indicatorSize = HarvestCircleTheme.shell.dimensions.iconSmall,
            )

        HarvestCircleControlSize.Medium ->
            HarvestCircleButtonMetrics(
                visualHeight = HarvestCircleTheme.shell.dimensions.controlMedium,
                minimumVisualWidth = 68.dp,
                horizontalPadding = HarvestCircleTheme.foundation.spacing.lg,
                indicatorSize = HarvestCircleTheme.shell.dimensions.iconMedium,
            )

        HarvestCircleControlSize.Large ->
            HarvestCircleButtonMetrics(
                visualHeight = HarvestCircleTheme.shell.dimensions.controlLarge,
                minimumVisualWidth = 86.dp,
                horizontalPadding = HarvestCircleTheme.foundation.spacing.xl,
                indicatorSize = HarvestCircleTheme.shell.dimensions.iconMedium,
            )
    }

@Composable
private fun variantTokens(variant: HarvestCircleButtonVariant): HarvestCircleActionStateColors =
    when (variant) {
        HarvestCircleButtonVariant.Primary -> HarvestCircleTheme.foundation.colors.action.primary
        HarvestCircleButtonVariant.Secondary -> HarvestCircleTheme.foundation.colors.action.secondary
        HarvestCircleButtonVariant.Ghost -> HarvestCircleTheme.foundation.colors.action.ghost
        HarvestCircleButtonVariant.Destructive -> HarvestCircleTheme.foundation.colors.action.destructive
    }

/**
 * Canonical HarvestCircle push button using pre-Liquid-Glass AppKit geometry and bezel treatment.
 *
 * Primary actions reproduce the supplied macOS default-submit control: a flat system-blue interior,
 * a one-pixel bright inner edge, and a three-dp muted-blue halo while pressed. The active halo is
 * interaction state, not keyboard focus state. [focusRing] remains independent and defaults to a
 * visible indicator when keyboard focus enters the control. Its transparent resting footprint
 * prevents layout movement on press.
 *
 * There is no Material ripple, tonal elevation, pill geometry, or Material button anatomy. Hover is
 * driven by a dedicated persistent hover source and changes immediately, avoiding transient edge
 * flashes.
 */
@Composable
public fun HarvestCircleButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    variant: HarvestCircleButtonVariant = HarvestCircleButtonVariant.Primary,
    size: HarvestCircleControlSize = HarvestCircleControlSize.Medium,
    enabled: Boolean = true,
    loading: Boolean = false,
    leadingIcon: (@Composable () -> Unit)? = null,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
    content: @Composable RowScope.() -> Unit,
) {
    val effectiveEnabled = enabled && !loading
    val sources = rememberHarvestCircleInteractionSources()
    val interactions = sources.harvestCircleInteractions(effectiveEnabled)
    val tokens = variantTokens(variant)
    val metrics = buttonMetrics(size)
    val loadingDescription = stringResource(Res.string.harvestcircle_loading)
    val nativePrimary = variant == HarvestCircleButtonVariant.Primary && effectiveEnabled
    val activeRingColor =
        if (nativePrimary && interactions.pressed) {
            HarvestCircleMacPrimarySubmitColors.ActiveRing
        } else {
            Color.Transparent
        }

    val container =
        when {
            nativePrimary && interactions.pressed -> HarvestCircleMacPrimarySubmitColors.Pressed
            nativePrimary && interactions.hovered -> HarvestCircleMacPrimarySubmitColors.Hover
            nativePrimary -> HarvestCircleMacPrimarySubmitColors.Rest
            !effectiveEnabled -> tokens.disabled
            interactions.pressed -> tokens.pressed
            interactions.hovered -> tokens.hover
            else -> tokens.rest
        }
    val contentColor =
        when {
            nativePrimary -> HarvestCircleMacPrimarySubmitColors.Content
            effectiveEnabled -> tokens.content
            else -> tokens.disabledContent
        }
    val isFilled = variant == HarvestCircleButtonVariant.Primary || variant == HarvestCircleButtonVariant.Destructive
    val borderColor =
        when {
            nativePrimary -> HarvestCircleMacPrimarySubmitColors.InnerEdge
            !effectiveEnabled -> HarvestCircleTheme.foundation.colors.border.subtle
            variant == HarvestCircleButtonVariant.Ghost && !interactions.hovered && !interactions.pressed ->
                Color.Transparent
            isFilled -> harvestCircleMacFilledBorder(container, HarvestCircleTheme.foundation.colors.isDark)
            else -> tokens.border
        }
    val shadow =
        if (
            effectiveEnabled &&
            !interactions.pressed &&
            variant != HarvestCircleButtonVariant.Ghost &&
            !nativePrimary
        ) {
            HarvestCircleTheme.component.elevations.raised
        } else {
            0.dp
        }
    val controlBrush: Brush =
        if (nativePrimary) {
            SolidColor(container)
        } else {
            harvestCircleMacControlBrush(container, emphasized = isFilled)
        }

    Box(
        modifier =
            modifier
                .defaultMinSize(minHeight = HarvestCircleTheme.shell.dimensions.minimumInteractive)
                .harvestCircleHoverable(sources = sources, enabled = effectiveEnabled)
                .clickable(
                    interactionSource = sources.activationSource,
                    indication = null,
                    enabled = effectiveEnabled,
                    role = Role.Button,
                    onClick = onClick,
                ).semantics {
                    if (loading) stateDescription = loadingDescription
                },
        contentAlignment = Alignment.Center,
    ) {
        HarvestCircleMacFocusFrame(
            focused = interactions.focused,
            focusRing = focusRing,
            shape = HarvestCircleTheme.foundation.shapes.control,
            ringShape = HarvestCircleTheme.foundation.shapes.controlFocusRing,
        ) {
            val bezel: @Composable () -> Unit = {
                HarvestCircleMacBezel(
                    modifier =
                        Modifier.defaultMinSize(
                            minWidth = metrics.minimumVisualWidth,
                            minHeight = metrics.visualHeight,
                        ),
                    brush = controlBrush,
                    border = BorderStroke(1.dp, borderColor),
                    shape = HarvestCircleTheme.foundation.shapes.control,
                    shadowElevation = shadow,
                ) {
                    ProvideHarvestCircleContentColor(contentColor) {
                        Row(
                            modifier = Modifier.padding(horizontal = metrics.horizontalPadding),
                            horizontalArrangement = Arrangement.Center,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            when {
                                loading -> {
                                    HarvestCircleMacSpinner(
                                        modifier = Modifier.size(metrics.indicatorSize),
                                        size = metrics.indicatorSize,
                                        color = contentColor,
                                    )
                                    Spacer(Modifier.width(HarvestCircleTheme.foundation.spacing.sm))
                                }

                                leadingIcon != null -> {
                                    leadingIcon()
                                    Spacer(Modifier.width(HarvestCircleTheme.foundation.spacing.sm))
                                }
                            }

                            content()
                        }
                    }
                }
            }

            if (nativePrimary) {
                Box(
                    modifier =
                        Modifier
                            .background(
                                color = activeRingColor,
                                shape = HarvestCircleTheme.foundation.shapes.controlFocusRing,
                            ).padding(HarvestCircleMacPrimarySubmitMetrics.RingWidth),
                    contentAlignment = Alignment.Center,
                ) {
                    bezel()
                }
            } else {
                bezel()
            }
        }
    }
}
