package org.harvestcircle.ui.shell

import org.harvestcircle.design.HarvestCircleDesign
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertNull

class ShellControlsTest {
    @Test
    fun focusChangesOnlyBorderAndRingAcrossEveryEnabledControlFamily() {
        listOf(HarvestCircleDesign.light, HarvestCircleDesign.dark).forEach { palette ->
            ShellButtonKind.entries.forEach { kind ->
                listOf(false, true).forEach { selected ->
                    listOf(false, true).forEach { hovered ->
                        listOf(false, true).forEach { pressed ->
                            val unfocused = resolveShellControlVisuals(kind, true, selected, false, pressed, hovered, palette)
                            val focused = resolveShellControlVisuals(kind, true, selected, true, pressed, hovered, palette)
                            assertEquals(unfocused.background, focused.background)
                            assertEquals(unfocused.foreground, focused.foreground)
                            assertEquals(palette.border, unfocused.border)
                            assertNull(unfocused.focusRing)
                            assertEquals(palette.focus, focused.border)
                            assertEquals(palette.focus, focused.focusRing)
                        }
                    }
                }
            }
        }
    }

    @Test
    fun interactionPreservesPrimaryAndDestructiveSemanticFamilies() {
        listOf(HarvestCircleDesign.light, HarvestCircleDesign.dark).forEach { palette ->
            listOf(false, true).forEach { hovered ->
                listOf(false, true).forEach { pressed ->
                    val primary = resolveShellControlVisuals(ShellButtonKind.Primary, true, false, false, pressed, hovered, palette)
                    val destructive =
                        resolveShellControlVisuals(ShellButtonKind.Destructive, true, false, false, pressed, hovered, palette)
                    assertEquals(palette.surface, primary.foreground)
                    assertEquals(palette.surface, destructive.foreground)
                    assertEquals(ShellControlBackground.Solid(palette.critical), destructive.background)
                    assertNotEquals(ShellControlBackground.Solid(palette.primary), destructive.background)
                    assertNotEquals(ShellControlBackground.Solid(palette.primaryHover), destructive.background)
                }
            }
            val hovered = resolveShellControlVisuals(ShellButtonKind.Primary, true, false, false, false, true, palette)
            val pressed = resolveShellControlVisuals(ShellButtonKind.Primary, true, false, false, true, true, palette)
            assertEquals(ShellControlBackground.Solid(palette.primaryHover), hovered.background)
            assertEquals(ShellControlBackground.Solid(palette.primary), pressed.background)
        }
    }

    @Test
    fun disabledControlsUseTheDisabledFamilyRegardlessOfOtherFlags() {
        listOf(HarvestCircleDesign.light, HarvestCircleDesign.dark).forEach { palette ->
            ShellButtonKind.entries.forEach { kind ->
                val visuals = resolveShellControlVisuals(kind, false, true, true, true, true, palette)
                assertEquals(ShellControlBackground.Solid(palette.surfaceSecondary), visuals.background)
                assertEquals(palette.textSecondary, visuals.foreground)
                assertEquals(palette.border, visuals.border)
                assertNull(visuals.focusRing)
            }
        }
    }
}
