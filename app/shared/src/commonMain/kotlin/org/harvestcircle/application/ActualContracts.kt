package org.harvestcircle.application

// Presentation data only. Native code owns source verification, comparisons,
// admission, physical balances, work retirement and current permission.
enum class HubActualSourceKind { Unknown, Possible, Reported }

data class HubActualSourceClaim(
    val traceId: String,
    val groupId: String? = null,
    val locationId: String? = null,
)

data class HubActualReportInput(
    val variantId: String? = null,
    val recipientId: String? = null,
    val wholePacks: UInt? = null,
    val sourceKind: HubActualSourceKind = HubActualSourceKind.Unknown,
    val sourceClaims: List<HubActualSourceClaim> = emptyList(),
    val occurrence: HubOccurrence = HubOccurrence(HubOccurrenceKind.Unknown),
    val observation: String,
)

enum class HubActualClockAssessment { Unknown, NotAfterRecordedTime, RecordingClockConflict }

data class HubActualReport(
    val id: String,
    val operationId: OperationId,
    val hubId: String,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val clockAssessment: HubActualClockAssessment,
    val reported: HubActualReportInput,
)

enum class HubActualReportFilter { Unresolved, All }

data class HubActualReportQuery(
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
    val filter: HubActualReportFilter = HubActualReportFilter.Unresolved,
)

data class HubActualReportRow(
    val report: HubActualReport,
    val reviewOperationId: OperationId?,
)

data class HubActualReportPage(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val rows: List<HubActualReportRow>,
)

data class HubActualDepartureInput(
    val reportId: String,
    val expectedStockRevision: ULong,
    val expectedCatalogRevision: ULong,
    val groupId: String,
    val expectedGroupRevision: ULong,
    val wholePacks: UInt,
    val recipientId: String,
    val occurrence: HubOccurrence,
    val sourceOperationId: OperationId,
    val sourceGroupId: String,
    val sourceEvidence: String,
    val quantityEvidence: String,
    val recipientEvidence: String,
    val timingEvidence: String,
    val duplicateReviewBasis: String,
)

data class HubActualLinkInput(
    val reportId: String,
    val knownOperationId: OperationId,
    val knownTraceId: String,
    val knownVariantId: String,
    val verification: String,
)

enum class HubActualHoldTiming { EstablishedBreach, BeforeHold, AfterRelease, TimingUnknown }

enum class HubActualHoldUncertainty {
    ScopeNotEstablished,
    MissingBirth,
    UnknownBirthTime,
    GroupNotPresentAtDeparture,
    AnchorNotPresentAtEffectiveTime,
    UncertainPhysicalAncestry,
    ExpandedPhysicalScope,
    AnchorCohortUnverified,
}

sealed interface HubActualHoldAssessment {
    val original: HubHold

    data class Unresolved(
        override val original: HubHold,
        val conditionalTiming: HubActualHoldTiming,
        val reason: HubActualHoldUncertainty,
    ) : HubActualHoldAssessment

    data class Verified(
        override val original: HubHold,
        val groupId: String,
        val occurrence: HubOccurrence,
        val lineageRevision: ULong,
        val timing: HubActualHoldTiming,
    ) : HubActualHoldAssessment
}

data class HubActualKnownLine(
    val groupId: String,
    val location: HubReferenceRecord,
    val wholePacks: UInt,
)

data class HubActualKnownDeparture(
    val operationId: OperationId,
    val actor: IdentityId,
    val hubId: String,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val traceId: String,
    val variant: HubReferenceRecord,
    val recipient: HubReferenceRecord,
    val occurrence: HubOccurrence,
    val recordedAt: UnixSeconds,
    val wholePacks: UInt,
    val lines: List<HubActualKnownLine>,
)

data class HubActualDuplicate(
    val operationId: OperationId,
    val report: HubActualReport,
    val known: HubActualKnownDeparture,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val verification: String,
)

// A current preview remains distinct from an immutable persisted receipt.
data class HubActualDeparturePreview(
    val report: HubActualReport,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val proposed: HubTraceActualDeparture,
    val historicalHolds: List<HubActualHoldAssessment>,
)

data class HubActualLinkPreview(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val proposed: HubActualDuplicate,
)

sealed interface HubActualReviewOriginal {
    data class Departure(
        val report: HubActualReport,
        val original: HubTraceActualDeparture,
        val historicalHolds: List<HubActualHoldAssessment>,
    ) : HubActualReviewOriginal

    data class Duplicate(
        val original: HubActualDuplicate,
    ) : HubActualReviewOriginal
}

data class HubActualReviewReceipt(
    val operationId: OperationId,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val original: HubActualReviewOriginal,
)

interface HarvestCircleActualRuntime {
    suspend fun reportActual(
        context: RequestContext,
        input: HubActualReportInput,
    ): HubActualReport

    suspend fun actualReports(query: HubActualReportQuery): HubActualReportPage

    suspend fun actualReportOperation(operation: OperationId): HubActualReport?

    suspend fun actualDeparturePreview(
        operation: OperationId,
        input: HubActualDepartureInput,
    ): HubActualDeparturePreview

    suspend fun reconcileActual(
        context: RequestContext,
        input: HubActualDepartureInput,
    ): HubActualReviewReceipt

    suspend fun actualLinkPreview(
        operation: OperationId,
        input: HubActualLinkInput,
    ): HubActualLinkPreview

    suspend fun linkActual(
        context: RequestContext,
        input: HubActualLinkInput,
    ): HubActualReviewReceipt

    suspend fun actualReviewOperation(operation: OperationId): HubActualReviewReceipt?
}
