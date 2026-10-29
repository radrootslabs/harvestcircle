package org.harvestcircle.application

// Native input and read projections. Rust owns comparison, history, work and effects.
data class HubOpeningInput(
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val variantId: String,
    val locationId: String,
    val wholePacks: UInt,
    val identification: HubPhysicalIdentification,
    val source: HubSourceInput,
    val historyReference: String?,
    val counted: HubOccurrence,
    val observation: String,
)

data class HubCountedStock(
    val population: HubStockPopulation,
    val group: HubPhysicalGroup,
    val acceptance: HubAcceptanceStatus,
)

data class HubOpeningReceipt(
    val operationId: OperationId,
    val stockRevision: ULong,
    val original: HubCountedStock,
)

data class HubCountObserveInput(
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val groupId: String,
    val expectedGroupRevision: ULong,
    val countedWholePacks: UInt,
    val cutoff: HubOccurrence,
    val observation: String,
)

data class HubCountPostInput(
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val countId: String,
    val reviewedActivity: Boolean,
    val surplusIdentification: HubPhysicalIdentification?,
    val reason: String,
)

data class HubCountObservation(
    val id: String,
    val operationId: OperationId,
    val recordedStockRevision: ULong,
    val hubId: String,
    val capturedStockRevision: ULong,
    val capturedCatalogRevision: ULong,
    val populationId: String,
    val groupId: String,
    val groupRevision: ULong,
    val product: HubReferenceRecord,
    val variant: HubReferenceRecord,
    val location: HubReferenceRecord,
    val recordedWholePacks: UInt,
    val countedWholePacks: UInt,
    val latestPhysicalRecordedAt: UnixSeconds,
    val previousReconciliationId: String?,
    val cutoff: HubOccurrence,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val observation: String,
)

sealed interface HubCountDifference {
    data object NoVariance : HubCountDifference

    data class Shortage(
        val wholePacks: UInt,
    ) : HubCountDifference

    data class Surplus(
        val wholePacks: UInt,
    ) : HubCountDifference
}

enum class HubCountBlockReason {
    ScopeChanged,
    NewerReconciliation,
    NotLatestObservation,
    CutoffUncertain,
    CutoffPrecedesRecordedActivity,
    UnresolvedActual,
    UncertainPriorOutcome,
}

data class HubCountReview(
    val observation: HubCountObservation,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val current: HubPhysicalGroup,
    val difference: HubCountDifference,
    val blockers: List<HubCountBlockReason>,
    val ready: Boolean,
)

data class HubCountWorkChange(
    val before: HubWorkRequirement,
    val after: HubWorkRequirement,
    val retiredWholePacks: UInt,
)

data class HubCountRemovedWork(
    val workId: String,
    val beforeProgress: HubWorkProgress,
    val beforeScopedWholePacks: UInt,
    val onHandAfter: UInt,
    val removedWholePacks: UInt,
    val removedProgress: HubWorkProgress,
)

data class HubCountPostReceipt(
    val id: String,
    val operationId: OperationId,
    val observation: HubCountObservation,
    val before: HubPhysicalGroup,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val product: HubReferenceRecord,
    val variant: HubReferenceRecord,
    val location: HubReferenceRecord,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val confirmedOnHandAt: UnixSeconds,
    val difference: HubCountDifference,
    val onHandAfter: UInt,
    val groupRevisionAfter: ULong,
    val workChanges: List<HubCountWorkChange>,
    val removedWork: List<HubCountRemovedWork>,
    val surplus: HubCountedStock?,
    val reason: String,
)

interface HarvestCircleCountRuntime {
    suspend fun recordOpening(
        context: RequestContext,
        input: HubOpeningInput,
    ): HubOpeningReceipt

    suspend fun openingOperation(operation: OperationId): HubOpeningReceipt?

    suspend fun observeCount(
        context: RequestContext,
        input: HubCountObserveInput,
    ): HubCountObservation

    suspend fun countObservationOperation(operation: OperationId): HubCountObservation?

    suspend fun countReview(countId: String): HubCountReview

    suspend fun postCount(
        context: RequestContext,
        input: HubCountPostInput,
    ): HubCountPostReceipt

    suspend fun countPostOperation(operation: OperationId): HubCountPostReceipt?
}
