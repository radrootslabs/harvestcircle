package org.harvestcircle.application

import org.harvestcircle.ffi.HubCountBlockReasonDto
import org.harvestcircle.ffi.HubCountDifferenceDto
import org.harvestcircle.ffi.HubCountObservationDto
import org.harvestcircle.ffi.HubCountObserveInputDto
import org.harvestcircle.ffi.HubCountPostInputDto
import org.harvestcircle.ffi.HubCountPostReceiptDto
import org.harvestcircle.ffi.HubCountReviewDto
import org.harvestcircle.ffi.HubCountedStockDto
import org.harvestcircle.ffi.HubOpeningInputDto
import org.harvestcircle.ffi.HubOpeningReceiptDto

internal fun HubOpeningInput.toNative() =
    HubOpeningInputDto(
        expectedStockRevision,
        expectedCatalogRevision,
        variantId,
        locationId,
        wholePacks,
        identification.toNative(),
        source.toNative(),
        historyReference,
        counted.toNative(),
        observation,
    )

internal fun HubCountObserveInput.toNative() =
    HubCountObserveInputDto(
        expectedStockRevision,
        expectedCatalogRevision,
        groupId,
        expectedGroupRevision,
        countedWholePacks,
        cutoff.toNative(),
        observation,
    )

internal fun HubCountPostInput.toNative() =
    HubCountPostInputDto(
        expectedStockRevision,
        expectedCatalogRevision,
        countId,
        reviewedActivity,
        surplusIdentification?.toNative(),
        reason,
    )

internal fun HubCountedStockDto.toModel() = HubCountedStock(population.toModel(), group.toModel(), acceptance.toModel())

internal fun HubOpeningReceiptDto.toModel() = HubOpeningReceipt(OperationId.from(operationId), stockRevision, original.toModel())

internal fun HubCountObservationDto.toModel() =
    HubCountObservation(
        id,
        OperationId.from(operationId),
        recordedStockRevision,
        hubId,
        capturedStockRevision,
        capturedCatalogRevision,
        populationId,
        groupId,
        groupRevision,
        product.toModel(),
        variant.toModel(),
        location.toModel(),
        recordedWholePacks,
        countedWholePacks,
        UnixSeconds(latestPhysicalRecordedAtUnixS),
        previousReconciliationId,
        cutoff.toModel(),
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        observation,
    )

private fun HubCountDifferenceDto.toModel() =
    when (this) {
        HubCountDifferenceDto.NoVariance -> HubCountDifference.NoVariance
        is HubCountDifferenceDto.Shortage -> HubCountDifference.Shortage(wholePacks)
        is HubCountDifferenceDto.Surplus -> HubCountDifference.Surplus(wholePacks)
    }

private fun HubCountBlockReasonDto.toModel() =
    when (this) {
        HubCountBlockReasonDto.SCOPE_CHANGED -> HubCountBlockReason.ScopeChanged
        HubCountBlockReasonDto.NEWER_RECONCILIATION -> HubCountBlockReason.NewerReconciliation
        HubCountBlockReasonDto.NOT_LATEST_OBSERVATION -> HubCountBlockReason.NotLatestObservation
        HubCountBlockReasonDto.CUTOFF_UNCERTAIN -> HubCountBlockReason.CutoffUncertain
        HubCountBlockReasonDto.CUTOFF_PRECEDES_RECORDED_ACTIVITY -> HubCountBlockReason.CutoffPrecedesRecordedActivity
        HubCountBlockReasonDto.UNRESOLVED_ACTUAL -> HubCountBlockReason.UnresolvedActual
        HubCountBlockReasonDto.UNCERTAIN_PRIOR_OUTCOME -> HubCountBlockReason.UncertainPriorOutcome
    }

internal fun HubCountReviewDto.toModel() =
    HubCountReview(
        observation.toModel(),
        stockRevision,
        catalogRevision,
        current.toModel(),
        difference.toModel(),
        blockers.map { it.toModel() },
        ready,
    )

internal fun HubCountPostReceiptDto.toModel() =
    HubCountPostReceipt(
        id,
        OperationId.from(operationId),
        observation.toModel(),
        before.toModel(),
        stockRevision,
        catalogRevision,
        product.toModel(),
        variant.toModel(),
        location.toModel(),
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        UnixSeconds(confirmedOnHandAtUnixS),
        difference.toModel(),
        onHandAfter,
        groupRevisionAfter,
        workChanges.map { HubCountWorkChange(it.before.toModel(), it.after.toModel(), it.retiredWholePacks) },
        removedWork.map {
            HubCountRemovedWork(
                it.workId,
                it.beforeProgress.toModel(),
                it.beforeScopedWholePacks,
                it.onHandAfter,
                it.removedWholePacks,
                it.removedProgress.toModel(),
            )
        },
        surplus?.toModel(),
        reason,
    )
