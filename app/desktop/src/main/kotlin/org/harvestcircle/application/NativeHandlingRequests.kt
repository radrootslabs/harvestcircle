package org.harvestcircle.application

import org.harvestcircle.ffi.HubAcceptanceInputDto
import org.harvestcircle.ffi.HubDiscrepancyOpenInputDto
import org.harvestcircle.ffi.HubDiscrepancyResolveInputDto
import org.harvestcircle.ffi.HubHandlingInputDto
import org.harvestcircle.ffi.HubHandlingQueryDto
import org.harvestcircle.ffi.HubHoldInputDto
import org.harvestcircle.ffi.HubHoldReleaseInputDto
import org.harvestcircle.ffi.HubMovementInputDto
import org.harvestcircle.ffi.HubNewWorkInputDto
import org.harvestcircle.ffi.HubPhysicalIdentificationDto
import org.harvestcircle.ffi.HubReplanInputDto
import org.harvestcircle.ffi.HubSplitInputDto

internal fun HubHandlingQuery.toNative() =
    HubHandlingQueryDto(
        expectedStockRevision,
        expectedCatalogRevision,
        offset,
        limit,
        traceId,
        openOnly,
        text,
    )

internal fun HubPhysicalIdentification.toNative() = HubPhysicalIdentificationDto(label, observation)

internal fun HubHandlingInput.toNative(): HubHandlingInputDto =
    when (this) {
        is HubHandlingInput.SetAcceptance ->
            HubHandlingInputDto.SetAcceptance(
                HubAcceptanceInputDto(
                    expectedStockRevision,
                    expectedCatalogRevision,
                    groupIds,
                    status.toNative(),
                    occurrence.toNative(),
                    observation,
                ),
            )
        is HubHandlingInput.PlaceHold ->
            HubHandlingInputDto.PlaceHold(
                HubHoldInputDto(expectedStockRevision, expectedCatalogRevision, groupIds, occurrence.toNative(), reason),
            )
        is HubHandlingInput.ReleaseHold ->
            HubHandlingInputDto.ReleaseHold(
                HubHoldReleaseInputDto(
                    expectedStockRevision,
                    expectedCatalogRevision,
                    holdId,
                    expectedHoldRevision,
                    occurrence.toNative(),
                    resolution,
                ),
            )
        is HubHandlingInput.Split ->
            HubHandlingInputDto.Split(
                HubSplitInputDto(
                    expectedStockRevision,
                    expectedCatalogRevision,
                    groupId,
                    expectedGroupRevision,
                    wholePacks,
                    identification.toNative(),
                    occurrence.toNative(),
                    observation,
                ),
            )
        is HubHandlingInput.Move ->
            HubHandlingInputDto.Move(
                HubMovementInputDto(
                    expectedStockRevision,
                    expectedCatalogRevision,
                    groupId,
                    expectedGroupRevision,
                    toLocationId,
                    wholePacks,
                    separatedPortion?.toNative(),
                    occurrence.toNative(),
                    observation,
                ),
            )
        is HubHandlingInput.Replan ->
            HubHandlingInputDto.Replan(
                HubReplanInputDto(
                    expectedStockRevision,
                    expectedCatalogRevision,
                    requirementId,
                    expectedWorkRevision,
                    targetLocationId,
                    observation,
                ),
            )
        is HubHandlingInput.NewWork ->
            HubHandlingInputDto.NewWork(
                HubNewWorkInputDto(
                    expectedStockRevision,
                    expectedCatalogRevision,
                    groupIds,
                    targetLocationId,
                    observation,
                ),
            )
        is HubHandlingInput.OpenDiscrepancy ->
            HubHandlingInputDto.OpenDiscrepancy(
                HubDiscrepancyOpenInputDto(
                    expectedStockRevision,
                    expectedCatalogRevision,
                    traceId,
                    groupId,
                    observation,
                ),
            )
        is HubHandlingInput.ResolveDiscrepancy ->
            HubHandlingInputDto.ResolveDiscrepancy(
                HubDiscrepancyResolveInputDto(
                    expectedStockRevision,
                    expectedCatalogRevision,
                    discrepancyId,
                    expectedRevision,
                    resolution,
                ),
            )
    }
