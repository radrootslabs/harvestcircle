package org.harvestcircle.application

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class ImportSecretDraftTest {
    @Test
    fun hcSl005DraftIsBoundedRedactedAndOneUse() {
        val secret = "nsec1" + "x".repeat(200)
        val draft = ImportSecretDraft.from(secret)

        assertEquals(MAX_IMPORT_SECRET_CHARS, draft.length)
        assertEquals("ImportSecretDraft([REDACTED])", draft.toString())
        assertTrue(secret !in draft.toString())
        assertEquals(secret.take(MAX_IMPORT_SECRET_CHARS), draft.take())
        assertFailsWith<IllegalStateException> { draft.take() }
        assertEquals("", draft.revealForDisplay())
    }

    @Test
    fun hcSl005ClearIsIdempotentAndDestroysAvailability() {
        val draft = ImportSecretDraft.from("distinctive-secret")

        draft.clear()
        draft.clear()

        assertEquals(0, draft.length)
        assertEquals("", draft.revealForDisplay())
        assertFailsWith<IllegalStateException> { draft.take() }
    }
}
