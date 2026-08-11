package org.harvestcircle.ui.shell

import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
    onShortcut: (ShellShortcut) -> Unit,
    content: @Composable () -> Unit,
) {
    Box(
        Modifier
            .fillMaxSize()
            .onPreviewKeyEvent { event ->
                event.toShellShortcut()?.let {
                    onShortcut(it)
                    true
                } ?: false
            }.testTag("shell-keyboard-host"),
    ) {
        content()
    }
}

@Composable
fun RouteFocusTarget(
    routeKey: String,
    label: String,
    restoreFocus: Boolean = true,
    content: @Composable () -> Unit,
) {
    val requester = remember(routeKey) { FocusRequester() }
    var modalWasOpen by remember(routeKey) { mutableStateOf(false) }
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
    LaunchedEffect(restoreFocus) {
        if (!restoreFocus) {
            modalWasOpen = true
        } else if (modalWasOpen) {
            requester.requestFocus()
            modalWasOpen = false
        }
    }
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
