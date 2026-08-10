package org.harvestcircle.application

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull

class GeneratedKeyBackupTest {
    @Test
    fun recoveryIsRedactedAndCanBeCleared() {
        val recovery = GeneratedKeyBackup("npub1generated", "nsec1generated")

        assertFalse(recovery.toString().contains("nsec1generated"))
        assertEquals("nsec1generated", recovery.revealNsec())
        recovery.clear()
        assertNull(recovery.revealNsecOrNull())
        assertFailsWith<IllegalStateException> { recovery.revealNsec() }
    }
}
