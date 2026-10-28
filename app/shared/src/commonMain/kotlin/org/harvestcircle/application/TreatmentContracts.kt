package org.harvestcircle.application

// Native data and readiness at a captured revision. Rust owns treatment decisions.
enum class HubAcceptanceStatus { ReviewRequired, Accepted, Rejected }

data class HubTreatmentScope(
    val traceId: String,
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
)

data class HubTreatmentQuery(
    val traceId: String,
    val expectedStockRevision: ULong? = null,
    val expectedCatalogRevision: ULong? = null,
    val offset: UInt = 0U,
    val limit: UInt = 64U,
)

data class HubPhysicalScopeAnchor(
    val groupId: String,
    val createdStockRevision: ULong,
    val capturedWholePacks: UInt,
)

data class HubCapturedPhysicalScope(
    val hubId: String,
    val traceId: String,
    val variantId: String,
    val capturedStockRevision: ULong,
    val anchors: List<HubPhysicalScopeAnchor>,
)

data class HubTreatmentStamp(
    val id: String,
    val stockRevision: ULong,
    val actor: IdentityId,
    val basis: HubHandlingBasis,
    val recordedAt: UnixSeconds,
    val occurrence: HubOccurrence,
    val observation: String,
)

data class HubAcceptanceDecision(
    val stamp: HubTreatmentStamp,
    val scope: HubCapturedPhysicalScope,
    val status: HubAcceptanceStatus,
)

data class HubHold(
    val stamp: HubTreatmentStamp,
    val scope: HubCapturedPhysicalScope,
    val revision: ULong,
    val release: HubTreatmentStamp?,
)

sealed interface HubTreatmentChange {
    data class Acceptance(
        val value: HubAcceptanceDecision,
    ) : HubTreatmentChange

    data class HoldPlaced(
        val value: HubHold,
    ) : HubTreatmentChange

    data class HoldReleased(
        val holdId: String,
        val release: HubTreatmentStamp,
    ) : HubTreatmentChange
}

data class HubTreatmentSummary(
    val onHand: UInt,
    val accepted: UInt,
    val reviewRequired: UInt,
    val rejected: UInt,
    val restricted: UInt,
    val departureEligible: UInt,
)

data class HubTreatmentOutcome(
    val change: HubTreatmentChange,
    val summary: HubTreatmentSummary,
    val affectedWholePacks: UInt,
    val historyCapacityReached: Boolean,
)

data class HubActionEligibility(
    val internalRelocation: Boolean,
    val observedSeparation: Boolean,
    val normalDeparture: Boolean,
    val disposition: Boolean,
)

data class HubGroupTreatment(
    val groupId: String,
    val wholePacks: UInt,
    val acceptance: HubAcceptanceStatus,
    val acceptanceDecisionId: String?,
    val activeHoldIds: List<String>,
    val actions: HubActionEligibility,
)

data class HubHoldRow(
    val hold: HubHold,
    val createdOperationId: OperationId,
    val releasedOperationId: OperationId?,
    val currentWholePacks: UInt,
)

data class HubTreatmentSnapshot(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val trace: HubReceiptTrace,
    val basis: HubHandlingBasis,
    val summary: HubTreatmentSummary,
    val historyCapacityReached: Boolean,
    val normalOperationsAvailable: Boolean,
    val groups: List<HubGroupTreatment>,
    val holds: List<HubHoldRow>,
)

data class HubTreatmentPage(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val offset: UInt,
    val total: UInt,
    val more: Boolean,
    val basis: HubHandlingBasis,
    val rows: List<HubHandlingReceipt>,
)
