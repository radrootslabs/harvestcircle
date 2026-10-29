package org.harvestcircle.application

// Native read projections. Rust owns scope selection, conservation and document content.
enum class HubTraceScope { All, Unresolved, Receipt, SourceImpact }

data class HubTraceSelection(
    val scope: HubTraceScope = HubTraceScope.All,
    val receiptId: String? = null,
    val sourceId: String? = null,
    val text: String = "",
)

data class HubTraceQuery(
    val selection: HubTraceSelection = HubTraceSelection(),
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
)

data class HubTraceRow(
    val trace: HubStockPopulation,
    val onHandWholePacks: UInt,
    val departedWholePacks: UInt,
)

data class HubTracePage(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val selection: HubTraceSelection,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val rows: List<HubTraceRow>,
)

data class HubTraceSnapshotQuery(
    val traceId: String,
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
)

data class HubTracePosition(
    val groupId: String,
    val groupRevision: ULong,
    val wholePacks: UInt,
    val location: HubReferenceRecord,
    val identification: HubPhysicalIdentification?,
    val acceptance: HubAcceptanceStatus,
    val acceptanceDecisionId: String?,
    val activeHoldIds: List<String>,
)

data class HubTraceExit(
    val groupId: String,
    val wholePacks: UInt,
    val departureId: String,
    val operationId: OperationId,
    val stockRevision: ULong,
    val preparationId: String,
    val identificationId: String,
    val recipient: HubReferenceRecord,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val occurrence: HubOccurrence,
    val observation: String,
)

data class HubTraceSnapshot(
    val hubId: String,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val trace: HubStockPopulation,
    val onHandWholePacks: UInt,
    val departedWholePacks: UInt,
    val onHand: List<HubTracePosition>,
    val exits: List<HubTraceExit>,
)

data class HubTraceHistoryQuery(
    val traceId: String,
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
)

enum class HubTraceHistoryKind {
    Receipt,
    CountedOpening,
    CountObserved,
    Move,
    Split,
    Acceptance,
    PlaceHold,
    ReleaseHold,
    Replan,
    NewWork,
    OpenDiscrepancy,
    ResolveDiscrepancy,
    PrepareIdentification,
    ReviseIdentification,
    IssueIdentification,
    ReplaceIdentification,
    MarkIdentificationUnused,
    Departure,
}

data class HubTraceHistoryEntry(
    val operationId: OperationId,
    val stockRevision: ULong,
    val kind: HubTraceHistoryKind,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val occurrence: HubOccurrence?,
    val observation: String,
    val resultId: String,
)

data class HubTraceHistoryPage(
    val traceId: String,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val rows: List<HubTraceHistoryEntry>,
)

sealed interface HubLocalOutputRequest {
    data class Identification(
        val preparationId: String,
        val identificationId: String,
    ) : HubLocalOutputRequest

    data class Trace(
        val traceId: String,
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
    ) : HubLocalOutputRequest
}

data class HubLocalOutput(
    val identity: String,
    val suggestedFilename: String,
    val html: String,
)

interface HarvestCircleTraceRuntime {
    suspend fun tracePage(query: HubTraceQuery): HubTracePage

    suspend fun traceSnapshot(query: HubTraceSnapshotQuery): HubTraceSnapshot

    suspend fun traceHistory(query: HubTraceHistoryQuery): HubTraceHistoryPage

    suspend fun localOutput(request: HubLocalOutputRequest): HubLocalOutput
}
