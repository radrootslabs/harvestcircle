package org.harvestcircle.application

import org.harvestcircle.ffi.HubGroupCreationKindDto
import org.harvestcircle.ffi.HubLineagePageDto
import org.harvestcircle.ffi.HubLineageQueryDto
import org.harvestcircle.ffi.HubLineageRowDto

internal fun HubLineageQuery.toNative() = HubLineageQueryDto(traceId, expectedStockRevision, expectedCatalogRevision, offset, limit)

private fun HubLineageRowDto.toModel() =
    HubLineageRow(
        id,
        parentId,
        identification?.toModel(),
        wholePacks,
        revision,
        location?.toModel(),
        createdStockRevision,
        when (createdKind) {
            HubGroupCreationKindDto.RECEIPT -> HubGroupCreationKind.Receipt
            HubGroupCreationKindDto.COUNTED_OPENING -> HubGroupCreationKind.CountedOpening
            HubGroupCreationKindDto.COUNTED_SURPLUS -> HubGroupCreationKind.CountedSurplus
            HubGroupCreationKindDto.MOVEMENT -> HubGroupCreationKind.Movement
            HubGroupCreationKindDto.SEPARATION -> HubGroupCreationKind.Separation
        },
        IdentityId.fromPublicKeyHex(createdBy),
        UnixSeconds(createdAtUnixS),
        OperationId.from(createdOperationId),
        occurrence.toModel(),
        observation,
    )

internal fun HubLineagePageDto.toModel() =
    HubLineagePage(
        stockRevision,
        catalogRevision,
        offset,
        total,
        more,
        product.toModel(),
        variant.toModel(),
        trace.toModel(),
        rows.map { it.toModel() },
    )
