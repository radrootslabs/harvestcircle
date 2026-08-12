package org.harvestcircle.ui.shell

import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.MotionPreference

val LocalShellAppearance = compositionLocalOf { AppearanceState() }

enum class ShellShortcutKey { Left, Right, K, One, Comma, Escape }

enum class ShellShortcut { Back, Forward, OpenNostrReference, Today, Settings, CloseOverlay }

fun resolveShellShortcut(
    key: ShellShortcutKey,
    alt: Boolean = false,
    controlOrMeta: Boolean = false,
): ShellShortcut? =
    when {
        alt && key == ShellShortcutKey.Left -> ShellShortcut.Back
        alt && key == ShellShortcutKey.Right -> ShellShortcut.Forward
        controlOrMeta && key == ShellShortcutKey.K -> ShellShortcut.OpenNostrReference
        controlOrMeta && key == ShellShortcutKey.One -> ShellShortcut.Today
        controlOrMeta && key == ShellShortcutKey.Comma -> ShellShortcut.Settings
        !alt && !controlOrMeta && key == ShellShortcutKey.Escape -> ShellShortcut.CloseOverlay
        else -> null
    }

fun nonessentialMotionEnabled(appearance: AppearanceState): Boolean = appearance.motion != MotionPreference.Reduced

@Composable
fun ShellKeyboardHost(
    modal: FoundationOverlay? = null,
    onShortcut: (ShellShortcut) -> Unit,
    content: @Composable () -> Unit,
) {
    Box(
        Modifier
            .fillMaxSize()
            .onPreviewKeyEvent { event ->
                val shortcut = event.toShellShortcut() ?: return@onPreviewKeyEvent false
                if (modal == null || shortcut == ShellShortcut.CloseOverlay) onShortcut(shortcut)
                true
            }.testTag("shell-keyboard-host"),
    ) {
        content()
    }
}

@Composable
fun RouteFocusTarget(
    routeKey: String,
    label: String,
    content: @Composable () -> Unit,
) {
    val requester = remember(routeKey) { FocusRequester() }
    val registry = LocalShellFocusRegistry.current
    DisposableEffect(registry, requester) {
        registry.register(ShellFocusTarget.RouteFallback, requester)
        onDispose { registry.unregister(ShellFocusTarget.RouteFallback, requester) }
    }
    Box(
        Modifier
            .fillMaxSize()
            .focusRequester(requester)
            .focusable()
            .semantics { contentDescription = label }
            .testTag("route-focus-target"),
    ) {
        content()
    }
    LaunchedEffect(routeKey) { requester.requestFocus() }
}

private fun KeyEvent.toShellShortcut(): ShellShortcut? {
    if (type != KeyEventType.KeyDown) return null
    val shellKey =
        when (key) {
            Key.DirectionLeft -> ShellShortcutKey.Left
            Key.DirectionRight -> ShellShortcutKey.Right
            Key.K -> ShellShortcutKey.K
            Key.One -> ShellShortcutKey.One
            Key.Comma -> ShellShortcutKey.Comma
            Key.Escape -> ShellShortcutKey.Escape
            else -> return null
        }
    return resolveShellShortcut(shellKey, alt = isAltPressed, controlOrMeta = isCtrlPressed || isMetaPressed)
}
