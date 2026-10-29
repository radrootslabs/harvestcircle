package org.harvestcircle.application

// Native projections and command transport. Rust owns every departure rule.
data class HubDepartureInput(
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val preparationId: String,
    val expectedPreparationRevision: ULong,
    val identificationId: String,
    val occurrence: HubOccurrence,
    val observation: String,
)

data class HubDepartureUse(
    val id: String,
    val operationId: OperationId,
    val preparationId: String,
    val identificationId: String,
    val stockRevision: ULong,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
)

data class HubDepartureLine(
    val groupId: String,
    val beforeGroupRevision: ULong,
    val afterGroupRevision: ULong,
    val wholePacks: UInt,
)

data class HubDepartureWorkChange(
    val before: HubWorkRequirement,
    val after: HubWorkRequirement,
    val retiredWholePacks: UInt,
)

data class HubDepartureReceipt(
    val id: String,
    val operationId: OperationId,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val occurrence: HubOccurrence,
    val observation: String,
    val paper: HubDispatchRecord,
    val lines: List<HubDepartureLine>,
    val workChanges: List<HubDepartureWorkChange>,
)

interface HarvestCircleDepartureRuntime {
    suspend fun departureReview(input: HubDepartureReviewInput): HubDepartureReview

    suspend fun applyDeparture(
        context: RequestContext,
        input: HubDepartureInput,
    ): HubDepartureReceipt

    suspend fun departureOperation(operation: OperationId): HubDepartureReceipt?
}

data class HubDepartureReviewInput(
    val preparationId: String,
    val expectedPreparationRevision: ULong,
    val identificationId: String,
)

data class HubDepartureReview(
    val request: HubDepartureReviewInput,
    val view: HubDispatchView,
    val ready: Boolean,
    val block: ApplicationProblem?,
    val proposedWorkChanges: List<HubDepartureWorkChange>,
)
