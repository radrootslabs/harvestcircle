package org.harvestcircle.application

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertSame

class ReferenceInputPolicyTest {
    @Test
    fun hcSc005PrivateKeyShapesAreRejectedCaseInsensitivelyAfterAsciiWhitespace() {
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
    fun hcSc006CharacterAndUtf8ByteLimitsAreEnforcedBeforeAdmission() {
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
    fun hcSl001ExactHexadecimalInteractiveReferencesAreAmbiguous() {
        val lowercase = "0123456789abcdef".repeat(4)
        val uppercase = lowercase.uppercase()
        val mixedCase = "0123456789aBcDeF".repeat(4)
        val knownPrivateKey = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"

        listOf(
            knownPrivateKey,
            lowercase,
            uppercase,
            mixedCase,
            "nostr:$lowercase",
            "NoStR:$mixedCase",
            " \t$lowercase\r\n",
            "\nNOSTR:$uppercase\t",
        ).forEach { raw ->
            assertSame(ReferenceInputAdmission.AmbiguousHex, ReferenceInputPolicy.admit(raw), raw)
        }
    }

    @Test
    fun hcSl001OnlyTheExactAsciiHexadecimalShapeIsAmbiguous() {
        val hex = "ab".repeat(32)

        listOf(
            hex.dropLast(1),
            hex + "a",
            "g" + hex.drop(1),
            "nostr: " + hex,
            "note1candidate",
            "nevent1candidate",
        ).forEach { raw ->
            assertIs<ReferenceInputAdmission.Accepted>(ReferenceInputPolicy.admit(raw), raw)
        }
    }

    @Test
    fun hcSc007AcceptedAdmissionAndEditIntentDoNotStringifyRawInput() {
        val distinctive = "distinctive-public-reference"
        val admission = assertIs<ReferenceInputAdmission.Accepted>(ReferenceInputPolicy.admit(distinctive))
        val intent = OverlayIntent.EditReference(distinctive)

        assertEquals("Accepted(value=[REDACTED])", admission.toString())
        assertEquals("EditReference(value=[REDACTED])", intent.toString())
    }
}
