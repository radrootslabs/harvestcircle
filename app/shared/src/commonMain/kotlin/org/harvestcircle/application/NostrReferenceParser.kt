package org.harvestcircle.application

enum class NostrReferenceClassification {
    Invalid,
    PrivateKeyRejected,
    EventId,
    PublicKey,
    Profile,
    Note,
    Event,
    Address,
}

data class NostrReferenceParseResult(
    val classification: NostrReferenceClassification,
    val canonicalReference: String?,
)

fun interface NostrReferenceParser {
    fun parse(raw: String): NostrReferenceParseResult
}

object RejectingNostrReferenceParser : NostrReferenceParser {
    override fun parse(raw: String): NostrReferenceParseResult = NostrReferenceParseResult(NostrReferenceClassification.Invalid, null)
}
