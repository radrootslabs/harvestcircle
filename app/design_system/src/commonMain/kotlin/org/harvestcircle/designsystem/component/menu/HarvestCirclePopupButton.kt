package org.harvestcircle.designsystem.component.menu

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import org.harvestcircle.designsystem.component.HarvestCircleButtonVariant
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleControlSize
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.component.HarvestCircleIconSize
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.action.HarvestCircleButton
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import org.harvestcircle.designsystem.internal.interaction.harvestCircleHoverable
import org.harvestcircle.designsystem.internal.interaction.harvestCircleInteractions
import org.harvestcircle.designsystem.internal.interaction.rememberHarvestCircleInteractionSources
import org.harvestcircle.designsystem.primitive.HarvestCircleIcon
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleInputMode
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** One value in a canonical macOS popup button menu. */
public data class HarvestCircleMenuOption<T>(
    public val value: T,
    public val label: String,
    public val enabled: Boolean = true,
)

private fun <T> nextEnabledIndex(
    options: List<HarvestCircleMenuOption<T>>,
    current: Int,
    direction: Int,
): Int {
    if (options.isEmpty()) return -1
    var candidate = current
    repeat(options.size) {
        candidate = (candidate + direction + options.size).mod(options.size)
        if (options[candidate].enabled) return candidate
    }
    return current
}

/** Canonical macOS popup button: a compact bezeled control opening a keyboard-operable menu. */
@Composable
public fun <T> HarvestCirclePopupButton(
    selectedValue: T,
    options: List<HarvestCircleMenuOption<T>>,
    onValueChange: (T) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    size: HarvestCircleControlSize = HarvestCircleControlSize.Medium,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
) {
    require(options.isNotEmpty()) { "HarvestCirclePopupButton requires at least one option" }

    var expanded by remember { mutableStateOf(false) }
    var anchorWidthPx by remember { mutableIntStateOf(0) }
    var activeIndex by remember(options, selectedValue) {
        mutableIntStateOf(options.indexOfFirst { it.value == selectedValue }.coerceAtLeast(0))
    }
    val selected = options.firstOrNull { it.value == selectedValue } ?: options.first()
    val menuItemMinimumHeight =
        if (HarvestCircleTheme.component.inputMode == HarvestCircleInputMode.Touch) {
            HarvestCircleTheme.shell.dimensions.minimumInteractive
        } else {
            HarvestCircleTheme.shell.dimensions.menuItemHeight
        }
    val density = LocalDensity.current
    val focusManager = LocalFocusManager.current
    val focusRequester = remember { FocusRequester() }
    val gapPx = with(density) { 4.dp.roundToPx() }
    val anchorWidth = with(density) { anchorWidthPx.toDp() }
    val positionProvider =
        remember(gapPx) {
            object : PopupPositionProvider {
                override fun calculatePosition(
                    anchorBounds: IntRect,
                    windowSize: IntSize,
                    layoutDirection: LayoutDirection,
                    popupContentSize: IntSize,
                ): IntOffset {
                    val preferredX =
                        if (layoutDirection == LayoutDirection.Ltr) {
                            anchorBounds.left
                        } else {
                            anchorBounds.right - popupContentSize.width
                        }
                    val x =
                        preferredX.coerceIn(
                            minimumValue = 0,
                            maximumValue = (windowSize.width - popupContentSize.width).coerceAtLeast(0),
                        )
                    val below = anchorBounds.bottom + gapPx
                    val y =
                        if (below + popupContentSize.height <= windowSize.height) {
                            below
                        } else {
                            (anchorBounds.top - popupContentSize.height - gapPx).coerceAtLeast(0)
                        }
                    return IntOffset(x, y)
                }
            }
        }

    Box(
        modifier =
            modifier.onGloballyPositioned {
                anchorWidthPx = it.size.width
            },
    ) {
        HarvestCircleButton(
            onClick = { expanded = !expanded },
            variant = HarvestCircleButtonVariant.Secondary,
            size = size,
            enabled = enabled,
            focusRing = focusRing,
        ) {
            HarvestCircleText(
                text = selected.label,
                role = HarvestCircleTextRole.Label,
                tone = HarvestCircleContentTone.Inherit,
                maxLines = 1,
            )
            Spacer(Modifier.width(HarvestCircleTheme.foundation.spacing.md))
            HarvestCircleIcon(
                resource = HarvestCircleIcons.ChevronDown,
                contentDescription = null,
                size = HarvestCircleIconSize.Small,
                tone = HarvestCircleContentTone.Inherit,
            )
        }

        if (expanded) {
            Popup(
                popupPositionProvider = positionProvider,
                onDismissRequest = {
                    expanded = false
                    focusManager.clearFocus()
                },
                properties = PopupProperties(focusable = true),
            ) {
                LaunchedEffect(Unit) {
                    focusRequester.requestFocus()
                }

                HarvestCircleSurface(
                    modifier =
                        Modifier
                            .widthIn(
                                min = anchorWidth.coerceAtLeast(160.dp),
                                max = 360.dp,
                            ).focusRequester(focusRequester)
                            .focusable()
                            .onPreviewKeyEvent { event ->
                                if (event.type != KeyEventType.KeyDown) {
                                    return@onPreviewKeyEvent false
                                }
                                when (event.key) {
                                    Key.DirectionDown -> {
                                        activeIndex = nextEnabledIndex(options, activeIndex, 1)
                                        true
                                    }

                                    Key.DirectionUp -> {
                                        activeIndex = nextEnabledIndex(options, activeIndex, -1)
                                        true
                                    }

                                    Key.Enter,
                                    Key.Spacebar,
                                    -> {
                                        options.getOrNull(activeIndex)?.takeIf { it.enabled }?.let {
                                            onValueChange(it.value)
                                            expanded = false
                                        }
                                        true
                                    }

                                    Key.MoveHome -> {
                                        activeIndex = options.indexOfFirst { it.enabled }.coerceAtLeast(0)
                                        true
                                    }

                                    Key.MoveEnd -> {
                                        activeIndex = options.indexOfLast { it.enabled }.coerceAtLeast(0)
                                        true
                                    }

                                    Key.Escape -> {
                                        expanded = false
                                        true
                                    }

                                    else -> false
                                }
                            },
                    role = HarvestCircleSurfaceRole.Overlay,
                    shape = HarvestCircleTheme.foundation.shapes.control,
                    border = BorderStroke(1.dp, HarvestCircleTheme.foundation.colors.border.default),
                    shadowElevation = HarvestCircleTheme.component.elevations.overlay,
                ) {
                    Column(modifier = Modifier.padding(HarvestCircleTheme.foundation.spacing.xs)) {
                        options.forEachIndexed { index, option ->
                            val sources = rememberHarvestCircleInteractionSources(option.value)
                            val interactions = sources.harvestCircleInteractions(option.enabled)
                            LaunchedEffect(interactions.hovered) {
                                if (interactions.hovered && option.enabled) {
                                    activeIndex = index
                                }
                            }
                            val active = index == activeIndex
                            val selectedOption = option.value == selectedValue
                            val container =
                                when {
                                    active && option.enabled -> HarvestCircleTheme.foundation.colors.action.primary.rest
                                    else -> HarvestCircleTheme.foundation.colors.surface.overlay
                                }
                            val contentColor =
                                when {
                                    !option.enabled -> HarvestCircleTheme.foundation.colors.content.disabled
                                    active -> HarvestCircleTheme.foundation.colors.content.inverse
                                    else -> HarvestCircleTheme.foundation.colors.content.primary
                                }

                            ProvideHarvestCircleContentColor(contentColor) {
                                Row(
                                    modifier =
                                        Modifier
                                            .fillMaxWidth()
                                            .defaultMinSize(minHeight = menuItemMinimumHeight)
                                            .clip(HarvestCircleTheme.foundation.shapes.control)
                                            .background(container)
                                            .harvestCircleHoverable(sources = sources, enabled = option.enabled)
                                            .clickable(
                                                interactionSource = sources.activationSource,
                                                indication = null,
                                                enabled = option.enabled,
                                                role = Role.Button,
                                                onClick = {
                                                    onValueChange(option.value)
                                                    expanded = false
                                                },
                                            ).padding(horizontal = HarvestCircleTheme.foundation.spacing.md),
                                    horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.sm),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Box(
                                        modifier = Modifier.width(HarvestCircleTheme.shell.dimensions.iconSmall),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        if (selectedOption) {
                                            HarvestCircleIcon(
                                                resource = HarvestCircleIcons.Check,
                                                contentDescription = null,
                                                size = HarvestCircleIconSize.Small,
                                                tone = HarvestCircleContentTone.Inherit,
                                            )
                                        }
                                    }
                                    HarvestCircleText(
                                        text = option.label,
                                        role = HarvestCircleTextRole.Body,
                                        tone = HarvestCircleContentTone.Inherit,
                                        maxLines = 1,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
