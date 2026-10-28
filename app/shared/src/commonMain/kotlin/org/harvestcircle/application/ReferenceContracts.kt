package org.harvestcircle.application

// Presentation/transport projections. Rust owns validation, revisions and effects.
enum class HubReferenceKind(
    val label: String,
) {
    Product("Products"),
    Pack("Original packs"),
    Party("Parties"),
    Location("Locations"),
}

enum class HubLocationPurpose(
    val label: String,
) {
    Receiving("Receiving"),
    Storage("Storage"),
    Other("Other"),
}

data class HubHandlingBasis(
    val id: String,
    val version: UInt,
    val label: String,
    val siteTimezone: String,
)

data class HubReferenceValue(
    val kind: HubReferenceKind,
    val label: String,
    val productId: String? = null,
    val nominalGrams: UInt? = null,
    val locationPurpose: HubLocationPurpose? = null,
)

data class HubReferenceRecord(
    val id: String,
    val revision: ULong,
    val value: HubReferenceValue,
)

data class HubReferenceQuery(
    val expectedCatalogRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
    val kind: HubReferenceKind? = null,
    val text: String = "",
)

data class HubReferencePage(
    val handlingBasis: HubHandlingBasis,
    val catalogRevision: ULong,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val records: List<HubReferenceRecord>,
)

data class HubReferenceEdit(
    val expectedCatalogRevision: ULong,
    val recordId: String? = null,
    val expectedRecordRevision: ULong? = null,
    val value: HubReferenceValue,
)

data class HubReferenceReceipt(
    val operationId: OperationId,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val catalogRevision: ULong,
    val previous: HubReferenceRecord?,
    val current: HubReferenceRecord,
)

interface HarvestCircleReferenceRuntime {
    suspend fun references(query: HubReferenceQuery): HubReferencePage

    suspend fun editReference(
        context: RequestContext,
        edit: HubReferenceEdit,
    ): HubReferenceReceipt

    suspend fun referenceOperation(operation: OperationId): HubReferenceReceipt?

    suspend fun referenceHistory(
        id: String,
        afterRevision: ULong,
        limit: UInt,
    ): List<HubReferenceReceipt>
}
