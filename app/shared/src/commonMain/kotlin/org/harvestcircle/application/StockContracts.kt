package org.harvestcircle.application

// Native transport and presentation projections. Rust owns all inventory rules.
enum class HubSourceCertainty { Unknown, Possible, Established }

enum class HubSourceKind { ProducerBatch, SupplierReference, Other }

enum class HubOccurrenceKind { Known, Interval, Unknown }

data class HubOccurrence(
    val kind: HubOccurrenceKind,
    val startUnixSeconds: Long? = null,
    val endUnixSeconds: Long? = null,
)

data class HubSourceInput(
    val existingSourceId: String? = null,
    val issuerId: String? = null,
    val kind: HubSourceKind? = null,
    val code: String? = null,
    val provenance: String? = null,
    val certainty: HubSourceCertainty = HubSourceCertainty.Unknown,
)

data class HubSource(
    val id: String?,
    val issuer: HubReferenceRecord?,
    val kind: HubSourceKind?,
    val code: String?,
    val provenance: String?,
    val certainty: HubSourceCertainty,
)

data class HubReceiptInput(
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val variantId: String,
    val supplierId: String?,
    val producerId: String?,
    val source: HubSourceInput,
    val wholePacks: UInt,
    val receivingId: String,
    val plannedLocationId: String?,
    val occurrence: HubOccurrence,
    val observation: String,
)

data class HubStockQuery(
    val expectedCatalogRevision: ULong? = null,
    val expectedStockRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
    val locationId: String? = null,
    val traceId: String? = null,
    val text: String = "",
)

sealed interface HubPopulationOrigin {
    data object Receipt : HubPopulationOrigin

    data class CountedOpening(
        val historyReference: String?,
    ) : HubPopulationOrigin

    data class CountedSurplus(
        val countId: String,
        val relatedPopulationId: String,
    ) : HubPopulationOrigin
}

data class HubStockPopulation(
    val id: String,
    val variant: HubReferenceRecord,
    val product: HubReferenceRecord,
    val supplier: HubReferenceRecord?,
    val producer: HubReferenceRecord?,
    val source: HubSource,
    val sourceNeedsReview: Boolean,
    val originalWholePacks: UInt,
    val initialLocation: HubReferenceRecord,
    val plannedLocation: HubReferenceRecord?,
    val occurrence: HubOccurrence,
    val recordedAt: UnixSeconds,
    val actor: IdentityId,
    val observation: String,
    val origin: HubPopulationOrigin = HubPopulationOrigin.Receipt,
)

data class HubStockPosition(
    val groupId: String,
    val traceId: String,
    val variantId: String,
    val locationId: String,
    val wholePacks: UInt,
    val revision: ULong,
)

data class HubQuantityWork(
    val required: UInt,
    val completed: UInt,
    val noLongerRequired: UInt,
    val remaining: UInt,
    val revision: ULong,
    val target: HubReferenceRecord?,
)

data class HubStockRow(
    val product: HubReferenceRecord,
    val position: HubStockPosition,
    val location: HubReferenceRecord,
    val trace: HubStockPopulation,
    val putAway: HubQuantityWork?,
)

data class HubStockPage(
    val handlingBasis: HubHandlingBasis,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val rows: List<HubStockRow>,
)

data class HubStockReceipt(
    val operationId: OperationId,
    val stockRevision: ULong,
    val trace: HubStockPopulation,
    val initialPosition: HubStockPosition,
    val initialPutAway: HubQuantityWork,
)

interface HarvestCircleStockRuntime {
    suspend fun stock(query: HubStockQuery): HubStockPage

    suspend fun receiveStock(
        context: RequestContext,
        input: HubReceiptInput,
    ): HubStockReceipt

    suspend fun receiptOperation(operation: OperationId): HubStockReceipt?
}
