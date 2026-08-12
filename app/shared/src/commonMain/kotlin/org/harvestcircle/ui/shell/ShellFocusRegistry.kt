package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import org.harvestcircle.application.ShellFocusTarget

internal class ShellFocusRegistry {
    private val requesters = mutableMapOf<ShellFocusTarget, FocusRequester>()

    fun register(
        target: ShellFocusTarget,
        requester: FocusRequester,
    ) {
        requesters[target] = requester
    }

    fun unregister(
        target: ShellFocusTarget,
        requester: FocusRequester,
    ) {
        if (requesters[target] == requester) requesters.remove(target)
    }

    fun request(target: ShellFocusTarget): Boolean = requesters[target]?.requestFocus() == true
}

internal val LocalShellFocusRegistry = compositionLocalOf { ShellFocusRegistry() }

@Composable
internal fun Modifier.shellFocusTarget(target: ShellFocusTarget): Modifier {
    val registry = LocalShellFocusRegistry.current
    val requester = remember(target) { FocusRequester() }
    DisposableEffect(registry, target, requester) {
        registry.register(target, requester)
        onDispose { registry.unregister(target, requester) }
    }
    return focusRequester(requester)
}

@Composable
internal fun ShellFocusRestorer(
    target: ShellFocusTarget?,
    fallback: ShellFocusTarget,
) {
    val registry = LocalShellFocusRegistry.current
    LaunchedEffect(target, fallback, registry) {
        if (target != null && !registry.request(target)) registry.request(fallback)
    }
}
