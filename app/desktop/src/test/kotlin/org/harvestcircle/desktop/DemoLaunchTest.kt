package org.harvestcircle.desktop

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull

class DemoLaunchTest {
    @Test
    fun ordinaryAndNamedLaunchAreExplicit() {
        assertNull(parseDemoRun(emptyArray()))
        assertEquals("hub-1", parseDemoRun(arrayOf("--demo", "hub-1")))
        assertEquals("a".repeat(48), parseDemoRun(arrayOf("--demo", "a".repeat(48))))
    }

    @Test
    fun invalidLaunchCannotBecomeOrdinaryStartup() {
        listOf(arrayOf("--demo", "valid", "extra"), arrayOf("--unknown"), arrayOf("--demo"))
            .forEach { assertFailsWith<IllegalArgumentException> { parseDemoRun(it) } }
        listOf("", "../normal", "/tmp/demo", "Name", "a b", "1demo", "a".repeat(49))
            .forEach { assertFailsWith<IllegalArgumentException> { parseDemoRun(arrayOf("--demo", it)) } }
    }
}
