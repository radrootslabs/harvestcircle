package org.harvestcircle.application

// Native checked read projections; client code does not derive stock quantities.
data class HubCountInterpretations(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val latestRecordedAt: UnixSeconds,
    val accounts: List<HubCountInterpretationAccount>,
    val explanations: List<HubCountExplanationMeaning>,
)

data class HubCountInterpretationAccount(
    val countOperationId: OperationId,
    val reconciliationId: String,
    val observationId: String,
    val traceId: String,
    val variantId: String,
    val anchorGroupId: String,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val confirmedOnHandAt: UnixSeconds,
    val grossShortageWholePacks: UInt,
    val explainedWholePacks: UInt,
    val unexplainedWholePacks: UInt,
    val classifications: List<HubCountClassificationMeaning>,
)

data class HubCountClassificationMeaning(
    val operationId: OperationId,
    val stockRevision: ULong,
    val explanationOperationId: OperationId,
    val beforeWholePacks: UInt,
    val afterWholePacks: UInt,
)

data class HubCountExplanationMeaning(
    val operationId: OperationId,
    val countOperationId: OperationId,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val occurrence: HubOccurrence,
    val originalRecipient: HubReferenceRecord,
    val interpretation: HubActualKnownInterpretation,
    val establishedReturnWholePacks: UInt,
)

data class HubCountReturnAssociation(
    val id: String,
    val operationId: OperationId,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val returnOperationId: OperationId,
    val returnStockRevision: ULong,
    val returnedId: String,
    val returnedPopulationId: String,
    val returnedGroupId: String,
    val explanationOperationId: OperationId,
    val explanationStockRevision: ULong,
    val explanationTraceId: String,
    val countOperationId: OperationId,
    val countStockRevision: ULong,
    val anchorGroupId: String,
    val variantId: String,
    val wholePacks: UInt,
    val returnOccurrence: HubOccurrence,
    val explanationOccurrence: HubOccurrence,
    val timing: HubReturnTiming,
)
