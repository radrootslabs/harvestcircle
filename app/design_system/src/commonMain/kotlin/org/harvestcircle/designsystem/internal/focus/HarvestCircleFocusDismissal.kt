package org.harvestcircle.designsystem.internal.focus

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusManager
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput

/**
 * Clears focus only when a pointer-down reaches this region without being consumed by a child.
 *
 * The final pointer-event pass runs after descendant buttons, text fields, selection controls,
 * menus, and other interactive elements have had the opportunity to consume the down event. An
 * unconsumed down therefore represents background space rather than another control.
 */
internal fun Modifier.harvestCircleClearFocusOnBackgroundPress(
    focusManager: FocusManager,
    enabled: Boolean,
    force: Boolean,
): Modifier =
    if (!enabled) {
        this
    } else {
        pointerInput(focusManager, force) {
            awaitEachGesture {
                awaitFirstDown(
                    requireUnconsumed = true,
                    pass = PointerEventPass.Final,
                )
                focusManager.clearFocus(force = force)
                waitForUpOrCancellation(pass = PointerEventPass.Final)
            }
        }
    }
