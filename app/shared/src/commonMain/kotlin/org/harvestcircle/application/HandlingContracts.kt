package org.harvestcircle.application

// Native projections and request transport. Rust owns physical and work decisions.
data class HubPhysicalIdentification(
    val label: String,
    val observation: String,
)

enum class HubWorkProgress { Pending, Completed, NoLongerRequired }

enum class HubWorkOrigin { Intake, Replan, NewHandling }

data class HubWorkRequirement(
    val id: String,
    val traceId: String,
    val variantId: String,
    val quantity: HubQuantityWork,
)

data class HubGroupWork(
    val requirement: HubWorkRequirement,
    val progress: HubWorkProgress,
)

data class HubPhysicalGroup(
    val id: String,
    val traceId: String,
    val variantId: String,
    val parentId: String?,
    val identification: HubPhysicalIdentification?,
    val locationId: String,
    val wholePacks: UInt,
    val revision: ULong,
    val causalFloor: UnixSeconds?,
    val work: List<HubGroupWork>,
)

data class HubGroupView(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val group: HubPhysicalGroup,
    val location: HubReferenceRecord,
    val product: HubReferenceRecord,
    val variant: HubReferenceRecord,
    val trace: HubStockPopulation,
)

data class HubHandlingQuery(
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
    val traceId: String? = null,
    val openOnly: Boolean = true,
    val text: String = "",
)

data class HubWorkRow(
    val work: HubWorkRequirement,
    val product: HubReferenceRecord,
    val variant: HubReferenceRecord,
    val sourceNeedsReview: Boolean,
    val pendingGroups: UInt,
    val origin: HubWorkOrigin,
    val observation: String,
    val createdBy: IdentityId,
    val createdAt: UnixSeconds,
    val createdOperation: OperationId,
)

data class HubWorkPage(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val rows: List<HubWorkRow>,
)

data class HubDiscrepancy(
    val id: String,
    val traceId: String,
    val groupId: String?,
    val observation: String,
    val revision: ULong,
    val resolution: String?,
)

data class HubDiscrepancyRow(
    val value: HubDiscrepancy,
    val product: HubReferenceRecord,
    val variant: HubReferenceRecord,
    val createdBy: IdentityId,
    val createdAt: UnixSeconds,
    val createdOperation: OperationId,
    val resolvedBy: IdentityId?,
    val resolvedAt: UnixSeconds?,
    val resolvedOperation: OperationId?,
)

data class HubDiscrepancyPage(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val rows: List<HubDiscrepancyRow>,
)

sealed interface HubHandlingInput {
    data class SetAcceptance(
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
        val groupIds: List<String>,
        val status: HubAcceptanceStatus,
        val occurrence: HubOccurrence,
        val observation: String,
    ) : HubHandlingInput

    data class PlaceHold(
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
        val groupIds: List<String>,
        val occurrence: HubOccurrence,
        val reason: String,
    ) : HubHandlingInput

    data class ReleaseHold(
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
        val holdId: String,
        val expectedHoldRevision: ULong,
        val occurrence: HubOccurrence,
        val resolution: String,
    ) : HubHandlingInput

    data class Split(
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
        val groupId: String,
        val expectedGroupRevision: ULong,
        val wholePacks: UInt,
        val identification: HubPhysicalIdentification,
        val occurrence: HubOccurrence,
        val observation: String,
    ) : HubHandlingInput

    data class Move(
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
        val groupId: String,
        val expectedGroupRevision: ULong,
        val toLocationId: String,
        val wholePacks: UInt,
        val separatedPortion: HubPhysicalIdentification?,
        val occurrence: HubOccurrence,
        val observation: String,
    ) : HubHandlingInput

    data class Replan(
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
        val requirementId: String,
        val expectedWorkRevision: ULong,
        val targetLocationId: String,
        val observation: String,
    ) : HubHandlingInput

    data class NewWork(
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
        val groupIds: List<String>,
        val targetLocationId: String,
        val observation: String,
    ) : HubHandlingInput

    data class OpenDiscrepancy(
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
        val traceId: String,
        val groupId: String?,
        val observation: String,
    ) : HubHandlingInput

    data class ResolveDiscrepancy(
        val expectedStockRevision: ULong,
        val expectedCatalogRevision: ULong,
        val discrepancyId: String,
        val expectedRevision: ULong,
        val resolution: String,
    ) : HubHandlingInput
}

data class HubWorkChange(
    val before: HubWorkRequirement?,
    val after: HubWorkRequirement,
)

data class HubActionGroup(
    val groupId: String,
    val beforeRevision: ULong,
    val wholePacks: UInt,
    val location: HubReferenceRecord,
)

sealed interface HubHandlingResult {
    data class Treatment(
        val value: HubTreatmentOutcome,
    ) : HubHandlingResult

    data class Separated(
        val selectedGroupId: String,
        val expectedGroupRevision: ULong,
        val sourceBeforePacks: UInt,
        val location: HubReferenceRecord,
        val wholePacks: UInt,
        val portionGroupId: String,
        val remainingPacks: UInt,
        val identification: HubPhysicalIdentification,
        val occurrence: HubOccurrence,
        val observation: String,
    ) : HubHandlingResult

    data class Moved(
        val selectedGroupId: String,
        val expectedGroupRevision: ULong,
        val sourceBeforeWholePacks: UInt,
        val from: HubReferenceRecord,
        val to: HubReferenceRecord,
        val wholePacks: UInt,
        val movedGroupId: String,
        val remainingWholePacks: UInt,
        val separatedPortion: HubPhysicalIdentification?,
        val occurrence: HubOccurrence,
        val observation: String,
        val workChanges: List<HubWorkChange>,
    ) : HubHandlingResult

    data class WorkChanged(
        val replacement: HubWorkRequirement,
        val previous: HubWorkChange?,
        val groups: List<HubActionGroup>,
        val observation: String,
    ) : HubHandlingResult

    data class DiscrepancyChanged(
        val value: HubDiscrepancy,
    ) : HubHandlingResult
}

enum class HubHandlingKind { Acceptance, PlaceHold, ReleaseHold, Split, Move, Replan, NewWork, OpenDiscrepancy, ResolveDiscrepancy }

data class HubHandlingReceipt(
    val operationId: OperationId,
    val kind: HubHandlingKind,
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val traceId: String,
    val actor: IdentityId,
    val recordedAt: UnixSeconds,
    val result: HubHandlingResult,
)

interface HarvestCircleHandlingRuntime {
    suspend fun treatment(scope: HubTreatmentScope): HubTreatmentSnapshot

    suspend fun treatmentHistory(query: HubTreatmentQuery): HubTreatmentPage

    suspend fun lineage(query: HubLineageQuery): HubLineagePage

    suspend fun group(id: String): HubGroupView

    suspend fun work(query: HubHandlingQuery): HubWorkPage

    suspend fun discrepancies(query: HubHandlingQuery): HubDiscrepancyPage

    suspend fun applyHandling(
        context: RequestContext,
        input: HubHandlingInput,
    ): HubHandlingReceipt

    suspend fun handlingOperation(operation: OperationId): HubHandlingReceipt?
}
