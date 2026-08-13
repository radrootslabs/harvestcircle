package org.harvestcircle.designsystem.shell

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption

public enum class HarvestCircleShellBannerTone { Information, Caution, Critical }

@Composable
public fun HarvestCircleShellBanner(
    title: String,
    message: String,
    modifier: Modifier = Modifier,
    tone: HarvestCircleShellBannerTone = HarvestCircleShellBannerTone.Information,
) {
    val colors = HarvestCircleShellPalette
    val border = if (tone == HarvestCircleShellBannerTone.Critical) colors.destructive else colors.border
    val shape =
        androidx.compose.foundation.shape
            .RoundedCornerShape(HarvestCircleShellMetrics.contentPanelRadius)
    Row(
        modifier =
            modifier
                .fillMaxWidth()
                .clip(shape)
                .background(if (tone == HarvestCircleShellBannerTone.Information) colors.accentSubtle else colors.raised)
                .border(BorderStroke(1.dp, border), shape)
                .padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            HarvestCircleShellText(title, role = HarvestCircleShellTextRole.BodyStrong)
            HarvestCircleShellText(message, color = colors.contentSecondary)
        }
    }
}

@Composable
public fun HarvestCircleShellDialogFrame(
    onDismissRequest: () -> Unit,
    title: String,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    val colors = HarvestCircleShellPalette
    val shape =
        androidx.compose.foundation.shape
            .RoundedCornerShape(14.dp)
    Dialog(onDismissRequest = onDismissRequest) {
        Column(
            modifier =
                modifier
                    .widthIn(min = 360.dp, max = 480.dp)
                    .clip(shape)
                    .background(colors.raised)
                    .border(BorderStroke(1.dp, colors.border), shape)
                    .semantics { paneTitle = title }
                    .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
            content = content,
        )
    }
}

@Composable
public fun <T> HarvestCircleShellMenuButton(
    selectedValue: T,
    options: List<HarvestCircleMenuOption<T>>,
    onValueChange: (T) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
) {
    require(options.isNotEmpty())
    var expanded by remember { mutableStateOf(false) }
    var anchorWidthPx by remember { mutableIntStateOf(0) }
    var activeIndex by remember(options, selectedValue) {
        mutableIntStateOf(options.indexOfFirst { it.value == selectedValue }.coerceAtLeast(0))
    }
    val density = LocalDensity.current
    val gapPx = with(density) { 4.dp.roundToPx() }
    val anchorWidth = with(density) { anchorWidthPx.toDp() }
    val requester = remember { FocusRequester() }
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
                        if (layoutDirection == LayoutDirection.Ltr) anchorBounds.left else anchorBounds.right - popupContentSize.width
                    val x = preferredX.coerceIn(0, (windowSize.width - popupContentSize.width).coerceAtLeast(0))
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
    Box(modifier.onGloballyPositioned { anchorWidthPx = it.size.width }) {
        HarvestCircleShellButton(label, { expanded = !expanded })
        if (expanded) {
            Popup(
                popupPositionProvider = positionProvider,
                onDismissRequest = { expanded = false },
                properties = PopupProperties(focusable = true),
            ) {
                val colors = HarvestCircleShellPalette
                val shape =
                    androidx.compose.foundation.shape
                        .RoundedCornerShape(HarvestCircleShellMetrics.controlRadius)
                LaunchedEffect(Unit) { requester.requestFocus() }
                Column(
                    Modifier
                        .widthIn(min = anchorWidth.coerceAtLeast(176.dp), max = 320.dp)
                        .clip(shape)
                        .background(colors.raised)
                        .border(BorderStroke(1.dp, colors.border), shape)
                        .padding(6.dp)
                        .focusRequester(requester)
                        .focusable()
                        .onPreviewKeyEvent { event ->
                            if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                            when (event.key) {
                                Key.DirectionDown -> {
                                    activeIndex = nextEnabled(options, activeIndex, 1)
                                    true
                                }
                                Key.DirectionUp -> {
                                    activeIndex = nextEnabled(options, activeIndex, -1)
                                    true
                                }
                                Key.Enter, Key.Spacebar -> {
                                    options.getOrNull(activeIndex)?.takeIf { it.enabled }?.let {
                                        onValueChange(it.value)
                                        expanded = false
                                    }
                                    true
                                }
                                Key.Escape -> {
                                    expanded = false
                                    true
                                }
                                else -> false
                            }
                        },
                ) {
                    options.forEachIndexed { index, option ->
                        val active = index == activeIndex
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .clip(
                                    androidx.compose.foundation.shape
                                        .RoundedCornerShape(8.dp),
                                ).background(if (active) colors.navigationSelected else colors.raised)
                                .clickable(enabled = option.enabled, role = Role.Button) {
                                    onValueChange(option.value)
                                    expanded = false
                                }.padding(horizontal = 10.dp, vertical = 8.dp),
                        ) {
                            HarvestCircleShellText(
                                option.label,
                                color = if (option.enabled) colors.contentPrimary else colors.contentDisabled,
                                maxLines = 1,
                            )
                        }
                    }
                }
            }
        }
    }
}

private fun <T> nextEnabled(
    options: List<HarvestCircleMenuOption<T>>,
    current: Int,
    direction: Int,
): Int {
    var candidate = current
    repeat(options.size) {
        candidate = (candidate + direction + options.size).mod(options.size)
        if (options[candidate].enabled) return candidate
    }
    return current
}
