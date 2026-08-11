package org.harvestcircle.application

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class NativeNostrReferenceParserTest {
    @Test
    fun canonicalPublicReferencesAreClassifiedByTheNativeParser() {
        for ((value, expected) in publicReferences) {
            val parsed = NativeNostrReferenceParser.parse(value)
            assertEquals(expected, parsed.classification)
            assertTrue(parsed.canonicalReference?.isNotEmpty() == true)
        }
    }

    @Test
    fun privateAndMalformedReferencesAreSeparatedWithoutReturningSensitiveInput() {
        val private = NativeNostrReferenceParser.parse(NSEC)
        assertEquals(NostrReferenceClassification.PrivateKeyRejected, private.classification)
        assertNull(private.canonicalReference)

        val invalid = NativeNostrReferenceParser.parse("note1qqqqqq")
        assertEquals(NostrReferenceClassification.Invalid, invalid.classification)
        assertNull(invalid.canonicalReference)
    }

    private companion object {
        const val NSEC = "nsec1j4c6269y9w0q2er2xjw8sv2ehyrtfxq3jwgdlxj6qfn8z4gjsq5qfvfk99"
        val publicReferences =
            listOf(
                "d94a3f4dd87b9a3b0bed183b32e916fa29c8020107845d1752d72697fe5309a5" to
                    NostrReferenceClassification.EventId,
                "npub14f8usejl26twx0dhuxjh9cas7keav9vr0v8nvtwtrjqx3vycc76qqh9nsy" to
                    NostrReferenceClassification.PublicKey,
                (
                    "nprofile1qqsrhuxx8l9ex335q7he0f09aej04zpazpl0ne2cgukyawd24mayt8gppemhxue69uhhytnc9e3k7mf" +
                        "0qyt8wumn8ghj7er2vfshxtnnv9jxkc3wvdhk6tclr7lsh"
                ) to
                    NostrReferenceClassification.Profile,
                "note1m99r7nwc0wdrkzldrqan96gklg5usqspq7z9696j6unf0ljnpxjspqfw99" to
                    NostrReferenceClassification.Note,
                (
                    "nevent1qqsdhet4232flykq3048jzc9msmaa3hnxuesxy3lnc33vd0wt9xwk6szyqewrqnkx4zsaweutf739s0cu7" +
                        "et29zrntqs5elw70vlm8zudr3y24sqsgy"
                ) to
                    NostrReferenceClassification.Event,
                "naddr1qqxnzd3exgersv33xymnsve3qgs8suecw4luyht9ekff89x4uacneapk8r5dyk0gmn6uwwurf6u9rusrqsqqqa282m3gxt" to
                    NostrReferenceClassification.Address,
            )
    }
}
