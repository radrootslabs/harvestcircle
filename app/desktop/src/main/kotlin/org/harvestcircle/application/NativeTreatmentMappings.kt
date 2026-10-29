package org.harvestcircle.application

import org.harvestcircle.ffi.HubAcceptanceStatusDto
import org.harvestcircle.ffi.HubCapturedPhysicalScopeDto
import org.harvestcircle.ffi.HubHoldDto
import org.harvestcircle.ffi.HubTreatmentChangeDto
import org.harvestcircle.ffi.HubTreatmentPageDto
import org.harvestcircle.ffi.HubTreatmentQueryDto
import org.harvestcircle.ffi.HubTreatmentResultDto
import org.harvestcircle.ffi.HubTreatmentScopeDto
import org.harvestcircle.ffi.HubTreatmentSnapshotDto
import org.harvestcircle.ffi.HubTreatmentStampDto
import org.harvestcircle.ffi.HubTreatmentSummaryDto

internal fun HubAcceptanceStatus.toNative() =
    when (this) {
        HubAcceptanceStatus.ReviewRequired -> HubAcceptanceStatusDto.REVIEW_REQUIRED
        HubAcceptanceStatus.Accepted -> HubAcceptanceStatusDto.ACCEPTED
        HubAcceptanceStatus.Rejected -> HubAcceptanceStatusDto.REJECTED
    }

internal fun HubAcceptanceStatusDto.toModel() =
    when (this) {
        HubAcceptanceStatusDto.REVIEW_REQUIRED -> HubAcceptanceStatus.ReviewRequired
        HubAcceptanceStatusDto.ACCEPTED -> HubAcceptanceStatus.Accepted
        HubAcceptanceStatusDto.REJECTED -> HubAcceptanceStatus.Rejected
    }

internal fun HubTreatmentScope.toNative() = HubTreatmentScopeDto(traceId, expectedStockRevision, expectedCatalogRevision)

internal fun HubTreatmentQuery.toNative() = HubTreatmentQueryDto(traceId, expectedStockRevision, expectedCatalogRevision, offset, limit)

private fun HubCapturedPhysicalScopeDto.toModel() =
    HubCapturedPhysicalScope(
        hubId,
        traceId,
        variantId,
        capturedStockRevision,
        anchors.map {
            HubPhysicalScopeAnchor(it.groupId, it.createdStockRevision, it.capturedWholePacks)
        },
    )

private fun HubTreatmentStampDto.toModel() =
    HubTreatmentStamp(
        id,
        stockRevision,
        IdentityId.fromPublicKeyHex(actorPublicKey),
        basis.toModel(),
        UnixSeconds(recordedAtUnixS),
        occurrence.toModel(),
        observation,
    )

private fun HubHoldDto.toModel() = HubHold(stamp.toModel(), scope.toModel(), revision, release?.toModel())

private fun HubTreatmentChangeDto.toModel(): HubTreatmentChange =
    when (this) {
        is HubTreatmentChangeDto.Acceptance ->
            HubTreatmentChange.Acceptance(
                HubAcceptanceDecision(value.stamp.toModel(), value.scope.toModel(), value.status.toModel()),
            )
        is HubTreatmentChangeDto.HoldPlaced -> HubTreatmentChange.HoldPlaced(value.toModel())
        is HubTreatmentChangeDto.HoldReleased -> HubTreatmentChange.HoldReleased(holdId, release.toModel())
    }

private fun HubTreatmentSummaryDto.toModel() =
    HubTreatmentSummary(onHand, accepted, reviewRequired, rejected, restricted, departureEligible)

internal fun HubTreatmentResultDto.toModel() =
    HubTreatmentOutcome(change.toModel(), summary.toModel(), affectedWholePacks, historyCapacityReached)

internal fun HubTreatmentSnapshotDto.toModel() =
    HubTreatmentSnapshot(
        stockRevision,
        catalogRevision,
        trace.toModel(),
        basis.toModel(),
        summary.toModel(),
        historyCapacityReached,
        normalOperationsAvailable,
        groups.map {
            HubGroupTreatment(
                it.groupId,
                it.wholePacks,
                it.acceptance.toModel(),
                it.acceptanceDecisionId,
                it.activeHoldIds,
                HubActionEligibility(
                    it.actions.internalRelocation,
                    it.actions.observedSeparation,
                    it.actions.normalDeparture,
                    it.actions.disposition,
                ),
            )
        },
        holds.map {
            HubHoldRow(
                it.hold.toModel(),
                OperationId.from(it.createdOperationId),
                it.releasedOperationId?.let(OperationId::from),
                it.currentWholePacks,
            )
        },
    )

internal fun HubTreatmentPageDto.toModel() =
    HubTreatmentPage(
        stockRevision,
        catalogRevision,
        offset,
        total,
        more,
        basis.toModel(),
        rows.map {
            it.toModel()
        },
    )
