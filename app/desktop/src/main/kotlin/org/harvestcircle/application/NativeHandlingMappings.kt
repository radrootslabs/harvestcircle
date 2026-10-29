package org.harvestcircle.application

import org.harvestcircle.ffi.HubDiscrepancyDto
import org.harvestcircle.ffi.HubDiscrepancyPageDto
import org.harvestcircle.ffi.HubGroupViewDto
import org.harvestcircle.ffi.HubHandlingKindDto
import org.harvestcircle.ffi.HubHandlingReceiptDto
import org.harvestcircle.ffi.HubHandlingResultDto
import org.harvestcircle.ffi.HubMovementResultDto
import org.harvestcircle.ffi.HubPhysicalIdentificationDto
import org.harvestcircle.ffi.HubSplitResultDto
import org.harvestcircle.ffi.HubWorkActionResultDto
import org.harvestcircle.ffi.HubWorkChangeDto
import org.harvestcircle.ffi.HubWorkOriginDto
import org.harvestcircle.ffi.HubWorkPageDto
import org.harvestcircle.ffi.HubWorkProgressDto
import org.harvestcircle.ffi.HubWorkRequirementDto

internal fun HubPhysicalIdentificationDto.toModel() = HubPhysicalIdentification(label, observation)

internal fun HubWorkRequirementDto.toModel() = HubWorkRequirement(id, traceId, variantId, quantity.toModel())

private fun HubWorkProgressDto.toModel() =
    when (this) {
        HubWorkProgressDto.PENDING -> HubWorkProgress.Pending
        HubWorkProgressDto.COMPLETED -> HubWorkProgress.Completed
        HubWorkProgressDto.NO_LONGER_REQUIRED -> HubWorkProgress.NoLongerRequired
    }

internal fun HubGroupViewDto.toModel() =
    HubGroupView(
        stockRevision,
        catalogRevision,
        HubPhysicalGroup(
            group.id,
            group.traceId,
            group.variantId,
            group.parentId,
            group.identification?.toModel(),
            group.locationId,
            group.wholePacks,
            group.revision,
            group.causalFloorUnixS?.let(::UnixSeconds),
            group.work.map { HubGroupWork(it.requirement.toModel(), it.progress.toModel()) },
        ),
        location.toModel(),
        product.toModel(),
        variant.toModel(),
        trace.toModel(),
    )

private fun HubWorkOriginDto.toModel() =
    when (this) {
        HubWorkOriginDto.INTAKE -> HubWorkOrigin.Intake
        HubWorkOriginDto.REPLAN -> HubWorkOrigin.Replan
        HubWorkOriginDto.NEW_HANDLING -> HubWorkOrigin.NewHandling
    }

internal fun HubWorkPageDto.toModel() =
    HubWorkPage(
        stockRevision,
        catalogRevision,
        offset,
        total,
        more,
        rows.map {
            HubWorkRow(
                it.work.toModel(),
                it.product.toModel(),
                it.variant.toModel(),
                it.sourceNeedsReview,
                it.pendingGroups,
                it.origin.toModel(),
                it.observation,
                IdentityId.fromPublicKeyHex(it.createdBy),
                UnixSeconds(it.createdAtUnixS),
                OperationId.from(it.createdOperationId),
            )
        },
    )

private fun HubDiscrepancyDto.toModel() = HubDiscrepancy(id, traceId, groupId, observation, revision, resolution)

internal fun HubDiscrepancyPageDto.toModel() =
    HubDiscrepancyPage(
        stockRevision,
        catalogRevision,
        offset,
        total,
        more,
        rows.map {
            HubDiscrepancyRow(
                it.value.toModel(),
                it.product.toModel(),
                it.variant.toModel(),
                IdentityId.fromPublicKeyHex(it.createdBy),
                UnixSeconds(it.createdAtUnixS),
                OperationId.from(it.createdOperationId),
                it.resolvedBy?.let(IdentityId::fromPublicKeyHex),
                it.resolvedAtUnixS?.let(::UnixSeconds),
                it.resolvedOperationId?.let(OperationId::from),
            )
        },
    )

private fun HubWorkChangeDto.toModel() = HubWorkChange(before?.toModel(), after.toModel())

private fun HubMovementResultDto.toModel() =
    HubHandlingResult.Moved(
        selectedGroupId,
        expectedGroupRevision,
        sourceBeforeWholePacks,
        from.toModel(),
        to.toModel(),
        wholePacks,
        movedGroupId,
        remainingWholePacks,
        separatedPortion?.toModel(),
        occurrence.toModel(),
        observation,
        workChanges.map { it.toModel() },
    )

private fun HubSplitResultDto.toModel() =
    HubHandlingResult.Separated(
        selectedGroupId,
        expectedGroupRevision,
        sourceBeforePacks,
        location.toModel(),
        wholePacks,
        portionGroupId,
        remainingPacks,
        identification.toModel(),
        occurrence.toModel(),
        observation,
    )

private fun HubWorkActionResultDto.toModel() =
    HubHandlingResult.WorkChanged(
        replacement.toModel(),
        previous?.toModel(),
        groups.map { HubActionGroup(it.groupId, it.beforeRevision, it.wholePacks, it.location.toModel()) },
        observation,
    )

private fun HubHandlingKindDto.toModel() =
    when (this) {
        HubHandlingKindDto.ACCEPTANCE -> HubHandlingKind.Acceptance
        HubHandlingKindDto.PLACE_HOLD -> HubHandlingKind.PlaceHold
        HubHandlingKindDto.RELEASE_HOLD -> HubHandlingKind.ReleaseHold
        HubHandlingKindDto.SPLIT -> HubHandlingKind.Split
        HubHandlingKindDto.MOVE -> HubHandlingKind.Move
        HubHandlingKindDto.REPLAN -> HubHandlingKind.Replan
        HubHandlingKindDto.NEW_WORK -> HubHandlingKind.NewWork
        HubHandlingKindDto.OPEN_DISCREPANCY -> HubHandlingKind.OpenDiscrepancy
        HubHandlingKindDto.RESOLVE_DISCREPANCY -> HubHandlingKind.ResolveDiscrepancy
    }

internal fun HubHandlingReceiptDto.toModel() =
    HubHandlingReceipt(
        OperationId.from(operationId),
        kind.toModel(),
        stockRevision,
        catalogRevision,
        traceId,
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        when (val original = result) {
            is HubHandlingResultDto.Treatment -> HubHandlingResult.Treatment(original.value.toModel())
            is HubHandlingResultDto.Separated -> original.value.toModel()
            is HubHandlingResultDto.Moved -> original.value.toModel()
            is HubHandlingResultDto.WorkChanged -> original.value.toModel()
            is HubHandlingResultDto.DiscrepancyChanged -> HubHandlingResult.DiscrepancyChanged(original.value.toModel())
        },
    )
