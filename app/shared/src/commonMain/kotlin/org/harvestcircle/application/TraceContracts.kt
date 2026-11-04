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
    val countedLossWholePacks: UInt = 0U,
    val disposedWholePacks: UInt = 0U,
    val actualDepartedWholePacks: UInt = 0U,
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

data class HubTraceCountLoss(
    val reconciliationId: String,
    val operationId: OperationId,
    val countId: String,
    val populationId: String,
    val groupId: String,
    val stockRevision: ULong,
    val wholePacks: UInt,
    val confirmedOnHandAt: UnixSeconds,
    val recordedAt: UnixSeconds,
    val actor: IdentityId,
    val reason: String,
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
    val countedLossWholePacks: UInt = 0U,
    val disposedWholePacks: UInt = 0U,
    val countLosses: List<HubTraceCountLoss> = emptyList(),
    val dispositions: List<HubTraceDisposition> = emptyList(),
    val actualDepartedWholePacks: UInt = 0U,
    val actualDepartures: List<HubTraceActualDeparture> = emptyList(),
    val quantityCorrections: List<HubTraceQuantityCorrection> = emptyList(),
)

data class HubTraceQuantityCorrection(
    val originalOperationId: OperationId,
    val originalGroupId: String,
    val originalWholePacks: UInt,
    val effectiveWholePacks: UInt,
    val establishedReturnWholePacks: UInt,
    val amendments: List<HubTraceQuantityAmendment>,
)

enum class HubTraceQuantityAmendmentKind { AdditionalDeparture, IdentifiedFoundRemainder }

data class HubTraceQuantityAmendment(
    val operationId: OperationId,
    val stockRevision: ULong,
    val beforeWholePacks: UInt,
    val afterWholePacks: UInt,
    val kind: HubTraceQuantityAmendmentKind,
    val deltaWholePacks: UInt,
)

data class HubTraceActualDeparture(
    val reportId: String,
    val operationId: OperationId,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val reportActor: IdentityId,
    val reportRecordedAt: UnixSeconds,
    val reportedOccurrence: HubOccurrence,
    val reportedClockConflict: Boolean,
    val originalObservation: String,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val occurrence: HubOccurrence,
    val wholePacks: UInt,
    val before: HubPhysicalGroup,
    val onHandAfterWholePacks: UInt,
    val afterGroupRevision: ULong,
    val afterCausalFloor: UnixSeconds?,
    val product: HubReferenceRecord,
    val variant: HubReferenceRecord,
    val recipient: HubReferenceRecord,
    val balancingLocation: HubReferenceRecord,
    val historicalLocation: HubReferenceRecord,
    val sourceOperationId: OperationId,
    val sourceGroupId: String,
    val sourceEvidence: String,
    val quantityEvidence: String,
    val recipientEvidence: String,
    val timingEvidence: String,
    val duplicateReviewBasis: String,
    val workChanges: List<HubDepartureWorkChange>,
)

data class HubTraceHistoryQuery(
    val traceId: String,
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
)

enum class HubTraceHistoryKind {
    Returned,
    Disposition,
    Receipt,
    CountedOpening,
    CountObserved,
    CountReconciled,
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
    ActualReported,
    ActualReconciled,
    ActualLinked,
    CorrectionObserved,
    FoundCorrected,
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

data class HubTraceDisposition(
    val id: String,
    val operationId: OperationId,
    val populationId: String,
    val groupId: String,
    val stockRevision: ULong,
    val wholePacks: UInt,
    val location: HubReferenceRecord,
    val kind: HubDispositionKind,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val occurrence: HubOccurrence,
    val observation: String,
)
