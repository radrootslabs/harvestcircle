package org.harvestcircle.application

import org.harvestcircle.ffi.HubHandlingBasisDto
import org.harvestcircle.ffi.HubLocationPurposeDto
import org.harvestcircle.ffi.HubReferenceEditDto
import org.harvestcircle.ffi.HubReferenceInputDto
import org.harvestcircle.ffi.HubReferenceKindDto
import org.harvestcircle.ffi.HubReferencePageDto
import org.harvestcircle.ffi.HubReferenceQueryDto
import org.harvestcircle.ffi.HubReferenceReceiptDto
import org.harvestcircle.ffi.HubReferenceRecordDto

internal fun HubReferenceKind.toNative(): HubReferenceKindDto =
    when (this) {
        HubReferenceKind.Product -> HubReferenceKindDto.PRODUCT
        HubReferenceKind.Pack -> HubReferenceKindDto.PACK
        HubReferenceKind.Party -> HubReferenceKindDto.PARTY
        HubReferenceKind.Location -> HubReferenceKindDto.LOCATION
    }

private fun HubReferenceKindDto.toModel(): HubReferenceKind =
    when (this) {
        HubReferenceKindDto.PRODUCT -> HubReferenceKind.Product
        HubReferenceKindDto.PACK -> HubReferenceKind.Pack
        HubReferenceKindDto.PARTY -> HubReferenceKind.Party
        HubReferenceKindDto.LOCATION -> HubReferenceKind.Location
    }

private fun HubLocationPurpose.toNative(): HubLocationPurposeDto =
    when (this) {
        HubLocationPurpose.Receiving -> HubLocationPurposeDto.RECEIVING
        HubLocationPurpose.Storage -> HubLocationPurposeDto.STORAGE
        HubLocationPurpose.Other -> HubLocationPurposeDto.OTHER
    }

private fun HubLocationPurposeDto.toModel(): HubLocationPurpose =
    when (this) {
        HubLocationPurposeDto.RECEIVING -> HubLocationPurpose.Receiving
        HubLocationPurposeDto.STORAGE -> HubLocationPurpose.Storage
        HubLocationPurposeDto.OTHER -> HubLocationPurpose.Other
    }

private fun HubReferenceValue.toNative(): HubReferenceInputDto =
    HubReferenceInputDto(kind.toNative(), label, productId, nominalGrams, locationPurpose?.toNative())

private fun HubReferenceInputDto.toModel(): HubReferenceValue =
    HubReferenceValue(kind.toModel(), label, productId, nominalGrams, locationPurpose?.toModel())

internal fun HubReferenceQuery.toNative(): HubReferenceQueryDto =
    HubReferenceQueryDto(expectedCatalogRevision, offset, limit, kind?.toNative(), text)

internal fun HubReferenceEdit.toNative(): HubReferenceEditDto =
    HubReferenceEditDto(expectedCatalogRevision, recordId, expectedRecordRevision, value.toNative())

internal fun HubReferenceRecordDto.toModel(): HubReferenceRecord = HubReferenceRecord(id, revision, value.toModel())

internal fun HubHandlingBasisDto.toModel(): HubHandlingBasis = HubHandlingBasis(id, version, label, siteTimezone)

internal fun HubReferencePageDto.toModel(): HubReferencePage =
    HubReferencePage(handlingBasis.toModel(), catalogRevision, offset, total, more, records.map { it.toModel() })

internal fun HubReferenceReceiptDto.toModel(): HubReferenceReceipt =
    HubReferenceReceipt(
        OperationId.from(operationId),
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        catalogRevision,
        previous?.toModel(),
        current.toModel(),
    )
