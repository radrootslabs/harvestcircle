package org.harvestcircle.application

import org.harvestcircle.ffi.HubDispositionInputDto
import org.harvestcircle.ffi.HubDispositionKindDto
import org.harvestcircle.ffi.HubDispositionReceiptDto
import org.harvestcircle.ffi.HubReturnClaimDto
import org.harvestcircle.ffi.HubReturnDepartureDto
import org.harvestcircle.ffi.HubReturnInputDto
import org.harvestcircle.ffi.HubReturnReceiptDto
import org.harvestcircle.ffi.HubReturnResolutionDto
import org.harvestcircle.ffi.HubReturnSourceDto
import org.harvestcircle.ffi.HubReturnTimingDto
import org.harvestcircle.ffi.HubUnresolvedReturnLinkDto

internal fun HubReturnTimingDto.toModel() =
    when (this) {
        HubReturnTimingDto.AFTER_OR_SAME -> HubReturnTiming.AfterOrSame
        HubReturnTimingDto.UNCERTAIN -> HubReturnTiming.Uncertain
        HubReturnTimingDto.CONTRADICTORY -> HubReturnTiming.Contradictory
    }

internal fun HubUnresolvedReturnLinkDto.toModel() =
    when (this) {
        HubUnresolvedReturnLinkDto.ORIGINAL_GROUP_UNKNOWN -> HubUnresolvedReturnLink.OriginalGroupUnknown
        HubUnresolvedReturnLinkDto.NO_MATCHING_VARIANT -> HubUnresolvedReturnLink.NoMatchingVariant
        HubUnresolvedReturnLinkDto.CLAIMED_GROUP_MISSING -> HubUnresolvedReturnLink.ClaimedGroupMissing
        HubUnresolvedReturnLinkDto.VARIANT_CONFLICT -> HubUnresolvedReturnLink.VariantConflict
        HubUnresolvedReturnLinkDto.QUANTITY_CONFLICT -> HubUnresolvedReturnLink.QuantityConflict
        HubUnresolvedReturnLinkDto.TIME_CONFLICT -> HubUnresolvedReturnLink.TimeConflict
    }

internal fun HubDispositionKindDto.toModel() =
    when (this) {
        HubDispositionKindDto.DESTROYED -> HubDispositionKind.Destroyed
        HubDispositionKindDto.OTHER_OBSERVED_REMOVAL -> HubDispositionKind.OtherObservedRemoval
    }

internal fun HubDispositionKind.toNative() =
    when (this) {
        HubDispositionKind.Destroyed -> HubDispositionKindDto.DESTROYED
        HubDispositionKind.OtherObservedRemoval -> HubDispositionKindDto.OTHER_OBSERVED_REMOVAL
    }

internal fun HubReturnResolutionDto.toModel() =
    when (this) {
        HubReturnResolutionDto.Unknown -> HubReturnResolution.Unknown
        HubReturnResolutionDto.Unrecorded -> HubReturnResolution.Unrecorded
        HubReturnResolutionDto.EstablishedScope -> HubReturnResolution.EstablishedScope
        is HubReturnResolutionDto.Unresolved -> HubReturnResolution.Unresolved(reason.toModel())
    }

internal fun HubReturnClaimDto.toModel() =
    HubReturnClaim(
        departureId,
        originalGroupId?.let { it },
    )

internal fun HubReturnClaim.toNative() =
    HubReturnClaimDto(
        departureId,
        originalGroupId?.let { it },
    )

internal fun HubReturnInput.toNative() =
    HubReturnInputDto(
        expectedStockRevision,
        expectedCatalogRevision,
        variantId,
        locationId,
        returnedById?.let { it },
        source.toNative(),
        wholePacks,
        identification.toNative(),
        plannedLocationId?.let { it },
        departure?.let { it.toNative() },
        occurrence.toNative(),
        observation,
    )

internal fun HubReturnDepartureDto.toModel() =
    HubReturnDeparture(
        departureId,
        OperationId.from(operationId),
        stockRevision,
        catalogRevision,
        identificationId,
        recipient.toModel(),
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        occurrence.toModel(),
    )

internal fun HubReturnSourceDto.toModel() =
    HubReturnSource(
        HubDispatchLine(
            original.groupId,
            original.groupRevision,
            original.wholePacks,
            original.identification?.toModel(),
            original.location.toModel(),
            original.product.toModel(),
            original.variant.toModel(),
            original.trace.toModel(),
        ),
        previouslyEstablishedReturnWholePacks,
    )

internal fun HubReturnReceiptDto.toModel() =
    HubReturnReceipt(
        id,
        OperationId.from(operationId),
        stockRevision,
        catalogRevision,
        population.toModel(),
        returnedBy?.let { it.toModel() },
        group.toModel(),
        intakeWork?.let { it.toModel() },
        acceptance.toModel(),
        claim?.let { it.toModel() },
        departure?.let { it.toModel() },
        claimedSource?.let { it.toModel() },
        plausibleSources.map { it.toModel() },
        timing.toModel(),
        resolution.toModel(),
    )

internal fun HubDispositionInput.toNative() =
    HubDispositionInputDto(
        expectedStockRevision,
        expectedCatalogRevision,
        groupId,
        expectedGroupRevision,
        wholePacks,
        kind.toNative(),
        occurrence.toNative(),
        observation,
    )

internal fun HubDispositionReceiptDto.toModel() =
    HubDispositionReceipt(
        id,
        OperationId.from(operationId),
        stockRevision,
        catalogRevision,
        before.toModel(),
        afterGroupRevision,
        product.toModel(),
        variant.toModel(),
        location.toModel(),
        kind.toModel(),
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        occurrence.toModel(),
        observation,
        workChanges.map { HubDepartureWorkChange(it.before.toModel(), it.after.toModel(), it.retiredWholePacks) },
    )
