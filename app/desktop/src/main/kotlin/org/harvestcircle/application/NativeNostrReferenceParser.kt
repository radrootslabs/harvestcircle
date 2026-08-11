package org.harvestcircle.application

import org.harvestcircle.ffi.NostrReferenceKindDto
import org.harvestcircle.ffi.classifyNostrReference

object NativeNostrReferenceParser : NostrReferenceParser {
    override fun parse(raw: String): NostrReferenceParseResult {
        val parsed = classifyNostrReference(raw)
        return NostrReferenceParseResult(
            classification =
                when (parsed.classification) {
                    NostrReferenceKindDto.INVALID -> NostrReferenceClassification.Invalid
                    NostrReferenceKindDto.PRIVATE_KEY_REJECTED -> NostrReferenceClassification.PrivateKeyRejected
                    NostrReferenceKindDto.EVENT_ID -> NostrReferenceClassification.EventId
                    NostrReferenceKindDto.PUBLIC_KEY -> NostrReferenceClassification.PublicKey
                    NostrReferenceKindDto.PROFILE -> NostrReferenceClassification.Profile
                    NostrReferenceKindDto.NOTE -> NostrReferenceClassification.Note
                    NostrReferenceKindDto.EVENT -> NostrReferenceClassification.Event
                    NostrReferenceKindDto.ADDRESS -> NostrReferenceClassification.Address
                },
            canonicalReference = parsed.canonicalReference,
        )
    }
}
