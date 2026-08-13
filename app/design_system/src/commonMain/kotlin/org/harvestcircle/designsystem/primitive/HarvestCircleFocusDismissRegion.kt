package org.harvestcircle.designsystem.primitive

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalFocusManager
import org.harvestcircle.designsystem.focus.HarvestCircleFocusDismissBehavior
import org.harvestcircle.designsystem.internal.focus.harvestCircleClearFocusOnBackgroundPress

/**
 * A non-visual region that optionally clears focus when its unhandled background is pressed.
 *
 * Descendant controls retain normal focus transfer because pointer-down events consumed by a child
 * are ignored. Use [HarvestCircleFocusDismissBehavior.KeepFocused] for editor canvases and other intentional
 * focus-retaining surfaces. Set [force] only when this region must override a descendant that
 * deliberately captured focus.
 */
@Composable
public fun HarvestCircleFocusDismissRegion(
    modifier: Modifier = Modifier,
    behavior: HarvestCircleFocusDismissBehavior = HarvestCircleFocusDismissBehavior.ClearOnBackgroundPress,
    force: Boolean = false,
    content: @Composable BoxScope.() -> Unit,
) {
    val focusManager = LocalFocusManager.current

    Box(
        modifier =
            modifier.harvestCircleClearFocusOnBackgroundPress(
                focusManager = focusManager,
                enabled = behavior == HarvestCircleFocusDismissBehavior.ClearOnBackgroundPress,
                force = force,
            ),
        content = content,
    )
}
