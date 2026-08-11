package org.harvestcircle.ui.shell

import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.MotionPreference
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ShellAccessibilityTest {
    @Test
    fun resolvesOnlyTheLockedEnabledShortcuts() {
        assertEquals(ShellShortcut.Back, resolveShellShortcut(ShellShortcutKey.Left, alt = true))
        assertEquals(ShellShortcut.Forward, resolveShellShortcut(ShellShortcutKey.Right, alt = true))
        assertEquals(ShellShortcut.OpenNostrReference, resolveShellShortcut(ShellShortcutKey.K, controlOrMeta = true))
        assertEquals(ShellShortcut.Today, resolveShellShortcut(ShellShortcutKey.One, controlOrMeta = true))
        assertEquals(ShellShortcut.Settings, resolveShellShortcut(ShellShortcutKey.Comma, controlOrMeta = true))
        assertEquals(ShellShortcut.CloseOverlay, resolveShellShortcut(ShellShortcutKey.Escape))
        assertNull(resolveShellShortcut(ShellShortcutKey.K))
        assertNull(resolveShellShortcut(ShellShortcutKey.Comma, alt = true))
    }

    @Test
    fun reducedMotionDisablesNonessentialTransitions() {
        assertTrue(nonessentialMotionEnabled(AppearanceState()))
        assertFalse(nonessentialMotionEnabled(AppearanceState(motion = MotionPreference.Reduced)))
    }
}
