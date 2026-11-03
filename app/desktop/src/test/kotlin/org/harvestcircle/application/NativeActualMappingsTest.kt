package org.harvestcircle.application

import org.harvestcircle.ffi.HubActualSourceKindDto
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class NativeActualMappingsTest {
    @Test
    fun reportedAndPossibleSourcesRetainNullableClaimsAndOriginalUnknownQuantity() {
        val claims = listOf(HubActualSourceClaim("01".repeat(16)), HubActualSourceClaim("02".repeat(16), "03".repeat(16), "04".repeat(16)))
        for ((kind, values) in listOf(
            HubActualSourceKind.Unknown to emptyList(),
            HubActualSourceKind.Possible to claims,
            HubActualSourceKind.Reported to claims.take(1),
        )) {
            val input =
                HubActualReportInput(
                    sourceKind = kind,
                    sourceClaims = values,
                    occurrence = HubOccurrence(HubOccurrenceKind.Known, 1_700_003_600L),
                    observation = "Original unverified Ω claim",
                )
            assertEquals(input, input.toNative().toModel())
        }
    }

    @Test
    fun inconsistentNativeSourceLayoutsAreRejectedRatherThanSilentlyDroppingClaims() {
        val original = HubActualReportInput(observation = "Unknown claim").toNative()
        assertFailsWith<IllegalArgumentException> { original.copy(sourceKind = HubActualSourceKindDto.POSSIBLE).toModel() }
        assertFailsWith<IllegalArgumentException> { original.copy(sourceKind = HubActualSourceKindDto.ESTABLISHED_CLAIM).toModel() }
        val claimed =
            HubActualReportInput(
                sourceKind = HubActualSourceKind.Reported,
                sourceClaims = listOf(HubActualSourceClaim("01".repeat(16))),
                observation = "Reported only",
            ).toNative()
        assertFailsWith<IllegalArgumentException> { claimed.copy(sourceKind = HubActualSourceKindDto.UNKNOWN).toModel() }
    }
}
