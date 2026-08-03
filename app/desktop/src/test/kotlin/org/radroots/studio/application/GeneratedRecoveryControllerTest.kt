package org.radroots.studio.application

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class GeneratedRecoveryControllerTest {
    @Test
    fun recoveryIsExclusiveRedactedAndClearedByAcknowledgement() {
        val controller = GeneratedRecoveryController()
        val recovery = controller.begin("npub1generated", "nsec1generated")

        assertFalse(recovery.toString().contains("nsec1generated"))
        assertFailsWith<IllegalStateException> {
            controller.begin("npub1other", "nsec1other")
        }
        assertEquals("nsec1generated", recovery.revealNsec())
        assertTrue(controller.acknowledge())
        assertFailsWith<IllegalStateException> { recovery.revealNsec() }
        assertFalse(controller.acknowledge())
    }

    @Test
    fun disposalClearsUnacknowledgedRecovery() {
        val controller = GeneratedRecoveryController()
        val recovery = controller.begin("npub1generated", "nsec1generated")

        controller.close()

        assertFailsWith<IllegalStateException> { recovery.revealNsec() }
    }
}
