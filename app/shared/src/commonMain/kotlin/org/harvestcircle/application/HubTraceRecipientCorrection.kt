package org.harvestcircle.application

// Native ordered whole-event interpretation. Null is explicitly unknown.
data class HubTraceRecipientCorrection(
    val operationId: OperationId,
    val previousOperationId: OperationId?,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val originalDepartureOperationId: OperationId,
    val originalRecipient: HubReferenceRecord,
    val originalPopulationIds: List<String>,
    val originalIssuedVersionIds: List<String>,
    val before: HubReferenceRecord?,
    val after: HubReferenceRecord?,
    val basis: String,
    val impact: List<HubCorrectionImpact>,
)
