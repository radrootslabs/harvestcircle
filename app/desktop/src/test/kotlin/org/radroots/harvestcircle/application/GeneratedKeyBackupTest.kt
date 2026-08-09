package org.radroots.harvestcircle.application

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse

class GeneratedKeyBackupTest {
    @Test
    fun recoveryIsRedactedAndCanBeCleared() {
        val recovery = GeneratedKeyBackup("npub1generated", "nsec1generated")

        assertFalse(recovery.toString().contains("nsec1generated"))
        assertEquals("nsec1generated", recovery.revealNsec())
        recovery.clear()
        assertFailsWith<IllegalStateException> { recovery.revealNsec() }
    }
}
