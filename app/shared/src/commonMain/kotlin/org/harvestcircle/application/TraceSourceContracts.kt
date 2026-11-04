package org.harvestcircle.application

// Native ordered interpretation and scope; local work never proves acknowledgement.
enum class HubCorrectionImpactKind { Population, Source, Departure, IssuedVersion, Recipient, UnresolvedSource, UnresolvedRecipient }

data class HubCorrectionImpact(
    val kind: HubCorrectionImpactKind,
    val targetId: String,
)

data class HubTraceSourceCorrection(
    val operationId: OperationId,
    val previousOperationId: OperationId?,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val before: HubSource,
    val after: HubSource,
    val basis: String,
    val impact: List<HubCorrectionImpact>,
)
