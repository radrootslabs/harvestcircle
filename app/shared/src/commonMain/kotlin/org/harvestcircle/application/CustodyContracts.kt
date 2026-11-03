package org.harvestcircle.application

// Transport and native originals only. Rust owns linkage, treatment and stock effects.
enum class HubReturnTiming { AfterOrSame, Uncertain, Contradictory }

enum class HubUnresolvedReturnLink {
    OriginalGroupUnknown,
    NoMatchingVariant,
    ClaimedGroupMissing,
    VariantConflict,
    QuantityConflict,
    TimeConflict,
}

enum class HubDispositionKind { Destroyed, OtherObservedRemoval }

sealed interface HubReturnResolution {
    data object Unknown : HubReturnResolution

    data object Unrecorded : HubReturnResolution

    data object EstablishedScope : HubReturnResolution

    data class Unresolved(
        val reason: HubUnresolvedReturnLink,
    ) : HubReturnResolution
}

data class HubReturnClaim(
    val departureId: String,
    val originalGroupId: String?,
)

data class HubReturnInput(
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val variantId: String,
    val locationId: String,
    val returnedById: String?,
    val source: HubSourceInput,
    val wholePacks: UInt,
    val identification: HubPhysicalIdentification,
    val plannedLocationId: String?,
    val departure: HubReturnClaim?,
    val occurrence: HubOccurrence,
    val observation: String,
)

data class HubReturnDeparture(
    val departureId: String,
    val operationId: OperationId,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val identificationId: String,
    val recipient: HubReferenceRecord,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val occurrence: HubOccurrence,
)

data class HubReturnSource(
    val original: HubDispatchLine,
    val previouslyEstablishedReturnWholePacks: UInt,
)

data class HubReturnReceipt(
    val id: String,
    val operationId: OperationId,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val population: HubStockPopulation,
    val returnedBy: HubReferenceRecord?,
    val group: HubPhysicalGroup,
    val intakeWork: HubWorkRequirement?,
    val acceptance: HubAcceptanceStatus,
    val claim: HubReturnClaim?,
    val departure: HubReturnDeparture?,
    val claimedSource: HubReturnSource?,
    val plausibleSources: List<HubReturnSource>,
    val timing: HubReturnTiming,
    val resolution: HubReturnResolution,
)

data class HubDispositionInput(
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val groupId: String,
    val expectedGroupRevision: ULong,
    val wholePacks: UInt,
    val kind: HubDispositionKind,
    val occurrence: HubOccurrence,
    val observation: String,
)

data class HubDispositionReceipt(
    val id: String,
    val operationId: OperationId,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val before: HubPhysicalGroup,
    val afterGroupRevision: ULong,
    val product: HubReferenceRecord,
    val variant: HubReferenceRecord,
    val location: HubReferenceRecord,
    val kind: HubDispositionKind,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val occurrence: HubOccurrence,
    val observation: String,
    val workChanges: List<HubDepartureWorkChange>,
)

interface HarvestCircleCustodyRuntime {
    suspend fun applyReturn(
        context: RequestContext,
        input: HubReturnInput,
    ): HubReturnReceipt

    suspend fun returnOperation(operation: OperationId): HubReturnReceipt?

    suspend fun applyDisposition(
        context: RequestContext,
        input: HubDispositionInput,
    ): HubDispositionReceipt

    suspend fun dispositionOperation(operation: OperationId): HubDispositionReceipt?
}
