package org.harvestcircle.application

import org.harvestcircle.ffi.HubDispatchActionInputDto
import org.harvestcircle.ffi.HubDispatchEditInputDto
import org.harvestcircle.ffi.HubDispatchFilterDto
import org.harvestcircle.ffi.HubDispatchInputDto
import org.harvestcircle.ffi.HubDispatchKindDto
import org.harvestcircle.ffi.HubDispatchLineInputDto
import org.harvestcircle.ffi.HubDispatchMaterialDto
import org.harvestcircle.ffi.HubDispatchMaterialInputDto
import org.harvestcircle.ffi.HubDispatchPageDto
import org.harvestcircle.ffi.HubDispatchQueryDto
import org.harvestcircle.ffi.HubDispatchReceiptDto
import org.harvestcircle.ffi.HubDispatchRecordDto
import org.harvestcircle.ffi.HubDispatchStampDto
import org.harvestcircle.ffi.HubDispatchStateDto
import org.harvestcircle.ffi.HubDispatchViewDto
import org.harvestcircle.ffi.HubIdentificationStatusDto
import org.harvestcircle.ffi.HubIdentificationVersionDto

internal fun HubDispatchQuery.toNative() =
    HubDispatchQueryDto(
        expectedStockRevision,
        expectedCatalogRevision,
        offset,
        limit,
        when (filter) {
            HubDispatchFilter.All -> HubDispatchFilterDto.ALL
            HubDispatchFilter.Draft -> HubDispatchFilterDto.DRAFT
            HubDispatchFilter.Issued -> HubDispatchFilterDto.ISSUED
            HubDispatchFilter.Unused -> HubDispatchFilterDto.UNUSED
            HubDispatchFilter.Departed -> HubDispatchFilterDto.DEPARTED
        },
        text,
    )

private fun HubDispatchMaterialInput.toNative() =
    HubDispatchMaterialInputDto(
        expectedStockRevision,
        expectedCatalogRevision,
        recipientId,
        lines.map {
            HubDispatchLineInputDto(it.groupId, it.expectedGroupRevision, it.wholePacks)
        },
        observation,
    )

private fun HubDispatchEditInput.toNative() = HubDispatchEditInputDto(preparationId, expectedPreparationRevision, material.toNative())

private fun HubDispatchActionInput.toNative() =
    HubDispatchActionInputDto(expectedStockRevision, expectedCatalogRevision, preparationId, expectedPreparationRevision, observation)

internal fun HubDispatchInput.toNative(): HubDispatchInputDto =
    when (this) {
        is HubDispatchInput.Prepare -> HubDispatchInputDto.Prepare(value.toNative())
        is HubDispatchInput.Revise -> HubDispatchInputDto.Revise(value.toNative())
        is HubDispatchInput.Issue -> HubDispatchInputDto.Issue(value.toNative())
        is HubDispatchInput.Replace -> HubDispatchInputDto.Replace(value.toNative())
        is HubDispatchInput.MarkUnused -> HubDispatchInputDto.MarkUnused(value.toNative())
    }

internal fun HubDispatchStateDto.toModel() =
    when (this) {
        HubDispatchStateDto.DRAFT -> HubDispatchState.Draft
        HubDispatchStateDto.ISSUED -> HubDispatchState.Issued
        HubDispatchStateDto.UNUSED -> HubDispatchState.Unused
        HubDispatchStateDto.DEPARTED -> HubDispatchState.Departed
    }

private fun HubDispatchStampDto.toModel() =
    HubDispatchStamp(
        OperationId.from(operationId),
        stockRevision,
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        observation,
    )

private fun HubDispatchMaterialDto.toModel() =
    HubDispatchMaterial(
        capturedStockRevision,
        capturedCatalogRevision,
        basis.toModel(),
        recipient.toModel(),
        lines.map {
            HubDispatchLine(
                it.groupId,
                it.groupRevision,
                it.wholePacks,
                it.identification?.toModel(),
                it.location.toModel(),
                it.product.toModel(),
                it.variant.toModel(),
                it.trace.toModel(),
            )
        },
        wholePacks,
        observation,
    )

private fun HubIdentificationVersionDto.toModel() =
    HubIdentificationVersion(
        id,
        number,
        previousId,
        material.toModel(),
        issued.toModel(),
        when (status) {
            HubIdentificationStatusDto.ISSUED -> HubIdentificationStatus.Issued
            HubIdentificationStatusDto.SUPERSEDED -> HubIdentificationStatus.Superseded
            HubIdentificationStatusDto.UNUSED -> HubIdentificationStatus.Unused
        },
        closed?.toModel(),
    )

internal fun HubDispatchRecordDto.toModel() =
    HubDispatchRecord(
        id,
        hubId,
        revision,
        state.toModel(),
        created.toModel(),
        updated.toModel(),
        draft?.toModel(),
        versions.map {
            it.toModel()
        },
    )

internal fun HubDispatchPageDto.toModel() =
    HubDispatchPage(
        stockRevision,
        catalogRevision,
        offset,
        total,
        more,
        rows.map {
            HubDispatchRow(
                it.id,
                it.revision,
                it.state.toModel(),
                it.recipient.toModel(),
                it.wholePacks,
                it.lineCount,
                it.versionCount,
                it.identificationId,
                it.updated.toModel(),
            )
        },
    )

internal fun HubDispatchViewDto.toModel() =
    HubDispatchView(stockRevision, catalogRevision, record.toModel(), state.toModel(), departure?.toModel())

internal fun HubDispatchReceiptDto.toModel() =
    HubDispatchReceipt(
        OperationId.from(operationId),
        when (kind) {
            HubDispatchKindDto.PREPARE -> HubDispatchKind.Prepare
            HubDispatchKindDto.REVISE -> HubDispatchKind.Revise
            HubDispatchKindDto.ISSUE -> HubDispatchKind.Issue
            HubDispatchKindDto.REPLACE -> HubDispatchKind.Replace
            HubDispatchKindDto.MARK_UNUSED -> HubDispatchKind.MarkUnused
        },
        catalogRevision,
        record.toModel(),
    )
