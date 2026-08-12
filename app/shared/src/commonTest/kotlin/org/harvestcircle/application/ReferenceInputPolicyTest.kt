package org.harvestcircle.application

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertSame

class ReferenceInputPolicyTest {
    @Test
    fun privateKeyShapesAreRejectedCaseInsensitivelyAfterAsciiWhitespace() {
        listOf(
            "nsec1",
            "nsec1partial",
            "NSEC1PARTIAL",
            "nostr:nsec1partial",
            "  nsec1partial",
            "\t\r\nNoStR:NsEc1partial",
        ).forEach { raw ->
            assertSame(ReferenceInputAdmission.PrivateKeyShaped, ReferenceInputPolicy.admit(raw))
        }
    }

    @Test
    fun ambiguousWhitespacePrefixFailsClosedWithinTheScanBudget() {
        val raw = " ".repeat(ReferenceInputPolicy.MAX_SENSITIVE_PREFIX_SCAN) + "public"
        assertSame(ReferenceInputAdmission.PrivateKeyShaped, ReferenceInputPolicy.admit(raw))
    }

    @Test
    fun characterAndUtf8ByteLimitsAreEnforcedBeforeAdmission() {
        val asciiLimit = "a".repeat(ReferenceInputPolicy.MAX_REFERENCE_CHARS)
        val multibyteOverflow = "é".repeat((ReferenceInputPolicy.MAX_REFERENCE_UTF8_BYTES / 2) + 1)

        assertEquals(asciiLimit, assertIs<ReferenceInputAdmission.Accepted>(ReferenceInputPolicy.admit(asciiLimit)).value)
        assertSame(
            ReferenceInputAdmission.TooLarge,
            ReferenceInputPolicy.admit(asciiLimit + "a"),
        )
        assertSame(ReferenceInputAdmission.TooLarge, ReferenceInputPolicy.admit(multibyteOverflow))
    }

    @Test
    fun acceptedAdmissionAndEditIntentDoNotStringifyRawInput() {
        val distinctive = "distinctive-public-reference"
        val admission = assertIs<ReferenceInputAdmission.Accepted>(ReferenceInputPolicy.admit(distinctive))
        val intent = OverlayIntent.EditReference(distinctive)

        assertEquals("Accepted(value=[REDACTED])", admission.toString())
        assertEquals("EditReference(value=[REDACTED])", intent.toString())
    }
}
