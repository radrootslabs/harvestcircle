package org.harvestcircle.ui.shell

import kotlin.test.Test
import kotlin.test.assertEquals

class ShellControlsTest {
    @Test
    fun visualStateUsesAccessibilityAndInteractionPriority() {
        assertEquals(ShellControlVisualState.Disabled, shellControlVisualState(false, true, true, true, true))
        assertEquals(ShellControlVisualState.Focused, shellControlVisualState(true, true, true, true, true))
        assertEquals(ShellControlVisualState.Pressed, shellControlVisualState(true, true, false, true, true))
        assertEquals(ShellControlVisualState.Selected, shellControlVisualState(true, true, false, false, true))
        assertEquals(ShellControlVisualState.Hovered, shellControlVisualState(true, false, false, false, true))
        assertEquals(ShellControlVisualState.Normal, shellControlVisualState(true, false, false, false, false))
    }
}
