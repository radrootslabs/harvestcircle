package org.harvestcircle.application

import org.harvestcircle.ffi.HubDepartureInputDto
import org.harvestcircle.ffi.HubDepartureReceiptDto
import org.harvestcircle.ffi.HubDepartureReviewDto
import org.harvestcircle.ffi.HubDepartureReviewInputDto
import org.harvestcircle.ffi.HubDepartureUseDto

internal fun HubDepartureInput.toNative() =
    HubDepartureInputDto(
        expectedStockRevision,
        expectedCatalogRevision,
        preparationId,
        expectedPreparationRevision,
        identificationId,
        occurrence.toNative(),
        observation,
    )

internal fun HubDepartureUseDto.toModel() =
    HubDepartureUse(
        id,
        OperationId.from(operationId),
        preparationId,
        identificationId,
        stockRevision,
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
    )

internal fun HubDepartureReceiptDto.toModel() =
    HubDepartureReceipt(
        id,
        OperationId.from(operationId),
        stockRevision,
        catalogRevision,
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        occurrence.toModel(),
        observation,
        paper.toModel(),
        lines.map { HubDepartureLine(it.groupId, it.beforeGroupRevision, it.afterGroupRevision, it.wholePacks) },
        workChanges.map { HubDepartureWorkChange(it.before.toModel(), it.after.toModel(), it.retiredWholePacks) },
    )

internal fun HubDepartureReviewInput.toNative() = HubDepartureReviewInputDto(preparationId, expectedPreparationRevision, identificationId)

internal fun HubDepartureReviewDto.toModel() =
    HubDepartureReview(
        HubDepartureReviewInput(request.preparationId, request.expectedPreparationRevision, request.identificationId),
        view.toModel(),
        ready,
        block?.toApplicationProblem(),
        proposedWorkChanges.map { HubDepartureWorkChange(it.before.toModel(), it.after.toModel(), it.retiredWholePacks) },
    )
