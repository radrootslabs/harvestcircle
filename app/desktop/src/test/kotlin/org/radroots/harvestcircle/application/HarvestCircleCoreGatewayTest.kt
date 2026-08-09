package org.radroots.harvestcircle.application

import org.radroots.harvestcircle.ffi.WireErrorCategory
import org.radroots.harvestcircle.ffi.WireErrorCode
import org.radroots.harvestcircle.ffi.WireRecoveryAction
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse

class HarvestCircleCoreGatewayTest {
    @Test
    fun unknownFailuresBecomeSanitizedTypedRejections() {
        val failure =
            IllegalStateException("sensitive detail")
                .toHarvestCircleCommandFailure("request-7")

        assertEquals(WireErrorCode.INTERNAL, failure.code)
        assertEquals(WireErrorCategory.INTERNAL, failure.category)
        assertEquals(WireRecoveryAction.NONE, failure.recoveryAction)
        assertEquals("request-7", failure.correlationId)
        assertEquals("The application command failed.", failure.safeMessage)
        assertFalse(failure.toString().contains("sensitive detail"))
    }
}
