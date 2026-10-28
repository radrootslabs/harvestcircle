package org.harvestcircle.application

// Native projections and transport; Rust owns preparation and identification rules.
enum class HubDispatchState { Draft, Issued, Unused }

enum class HubDispatchFilter { All, Draft, Issued, Unused }

enum class HubDispatchKind { Prepare, Revise, Issue, Replace, MarkUnused }

enum class HubIdentificationStatus { Issued, Superseded, Unused }

data class HubDispatchQuery(
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
    val filter: HubDispatchFilter = HubDispatchFilter.All,
    val text: String = "",
)

data class HubDispatchLineInput(
    val groupId: String,
    val expectedGroupRevision: ULong,
    val wholePacks: UInt,
)

data class HubDispatchMaterialInput(
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val recipientId: String,
    val lines: List<HubDispatchLineInput>,
    val observation: String,
)

data class HubDispatchEditInput(
    val preparationId: String,
    val expectedPreparationRevision: ULong,
    val material: HubDispatchMaterialInput,
)

data class HubDispatchActionInput(
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val preparationId: String,
    val expectedPreparationRevision: ULong,
    val observation: String,
)

sealed interface HubDispatchInput {
    data class Prepare(
        val value: HubDispatchMaterialInput,
    ) : HubDispatchInput

    data class Revise(
        val value: HubDispatchEditInput,
    ) : HubDispatchInput

    data class Issue(
        val value: HubDispatchActionInput,
    ) : HubDispatchInput

    data class Replace(
        val value: HubDispatchEditInput,
    ) : HubDispatchInput

    data class MarkUnused(
        val value: HubDispatchActionInput,
    ) : HubDispatchInput
}

data class HubDispatchStamp(
    val operationId: OperationId,
    val stockRevision: ULong,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val observation: String,
)

data class HubDispatchLine(
    val groupId: String,
    val groupRevision: ULong,
    val wholePacks: UInt,
    val identification: HubPhysicalIdentification?,
    val location: HubReferenceRecord,
    val product: HubReferenceRecord,
    val variant: HubReferenceRecord,
    val trace: HubReceiptTrace,
)

data class HubDispatchMaterial(
    val capturedStockRevision: ULong,
    val capturedCatalogRevision: ULong,
    val basis: HubHandlingBasis,
    val recipient: HubReferenceRecord,
    val lines: List<HubDispatchLine>,
    val wholePacks: UInt,
    val observation: String,
)

data class HubIdentificationVersion(
    val id: String,
    val number: UInt,
    val previousId: String?,
    val material: HubDispatchMaterial,
    val issued: HubDispatchStamp,
    val status: HubIdentificationStatus,
    val closed: HubDispatchStamp?,
)

data class HubDispatchRecord(
    val id: String,
    val hubId: String,
    val revision: ULong,
    val state: HubDispatchState,
    val created: HubDispatchStamp,
    val updated: HubDispatchStamp,
    val draft: HubDispatchMaterial?,
    val versions: List<HubIdentificationVersion>,
)

data class HubDispatchRow(
    val id: String,
    val revision: ULong,
    val state: HubDispatchState,
    val recipient: HubReferenceRecord,
    val wholePacks: UInt,
    val lineCount: UInt,
    val versionCount: UInt,
    val identificationId: String?,
    val updated: HubDispatchStamp,
)

data class HubDispatchPage(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val rows: List<HubDispatchRow>,
)

data class HubDispatchView(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val record: HubDispatchRecord,
)

data class HubDispatchReceipt(
    val operationId: OperationId,
    val kind: HubDispatchKind,
    val catalogRevision: ULong,
    val record: HubDispatchRecord,
)

interface HarvestCircleDispatchRuntime {
    suspend fun dispatchPage(query: HubDispatchQuery): HubDispatchPage

    suspend fun dispatchRecord(id: String): HubDispatchView

    suspend fun applyDispatch(
        context: RequestContext,
        input: HubDispatchInput,
    ): HubDispatchReceipt

    suspend fun dispatchOperation(operation: OperationId): HubDispatchReceipt?
}
