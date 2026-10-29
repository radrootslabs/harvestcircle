package org.harvestcircle.application

// Native projections only. Rust resolves creation lineage and physical scope.
enum class HubGroupCreationKind { Receipt, CountedOpening, Movement, Separation }

data class HubLineageQuery(
    val traceId: String,
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
)

data class HubLineageRow(
    val id: String,
    val parentId: String?,
    val identification: HubPhysicalIdentification?,
    val wholePacks: UInt,
    val revision: ULong,
    val location: HubReferenceRecord?,
    val createdStockRevision: ULong,
    val createdKind: HubGroupCreationKind,
    val createdBy: IdentityId,
    val createdAt: UnixSeconds,
    val createdOperationId: OperationId,
    val occurrence: HubOccurrence,
    val observation: String,
)

data class HubLineagePage(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val product: HubReferenceRecord,
    val variant: HubReferenceRecord,
    val trace: HubStockPopulation,
    val rows: List<HubLineageRow>,
)
