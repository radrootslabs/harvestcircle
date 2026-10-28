package org.harvestcircle.application

import org.harvestcircle.ffi.HubOccurrenceDto
import org.harvestcircle.ffi.HubOccurrenceKindDto
import org.harvestcircle.ffi.HubQuantityWorkDto
import org.harvestcircle.ffi.HubReceiptInputDto
import org.harvestcircle.ffi.HubReceiptTraceDto
import org.harvestcircle.ffi.HubSourceCertaintyDto
import org.harvestcircle.ffi.HubSourceDto
import org.harvestcircle.ffi.HubSourceInputDto
import org.harvestcircle.ffi.HubSourceKindDto
import org.harvestcircle.ffi.HubStockPageDto
import org.harvestcircle.ffi.HubStockPositionDto
import org.harvestcircle.ffi.HubStockQueryDto
import org.harvestcircle.ffi.HubStockReceiptDto

private fun HubSourceCertainty.toNative() =
    when (this) {
        HubSourceCertainty.Unknown -> HubSourceCertaintyDto.UNKNOWN
        HubSourceCertainty.Possible -> HubSourceCertaintyDto.POSSIBLE
        HubSourceCertainty.Established -> HubSourceCertaintyDto.ESTABLISHED
    }

private fun HubSourceCertaintyDto.toModel() =
    when (this) {
        HubSourceCertaintyDto.UNKNOWN -> HubSourceCertainty.Unknown
        HubSourceCertaintyDto.POSSIBLE -> HubSourceCertainty.Possible
        HubSourceCertaintyDto.ESTABLISHED -> HubSourceCertainty.Established
    }

private fun HubSourceKind.toNative() =
    when (this) {
        HubSourceKind.ProducerBatch -> HubSourceKindDto.PRODUCER_BATCH
        HubSourceKind.SupplierReference -> HubSourceKindDto.SUPPLIER_REFERENCE
        HubSourceKind.Other -> HubSourceKindDto.OTHER
    }

private fun HubSourceKindDto.toModel() =
    when (this) {
        HubSourceKindDto.PRODUCER_BATCH -> HubSourceKind.ProducerBatch
        HubSourceKindDto.SUPPLIER_REFERENCE -> HubSourceKind.SupplierReference
        HubSourceKindDto.OTHER -> HubSourceKind.Other
    }

internal fun HubOccurrence.toNative() =
    HubOccurrenceDto(
        when (kind) {
            HubOccurrenceKind.Known -> HubOccurrenceKindDto.KNOWN
            HubOccurrenceKind.Interval -> HubOccurrenceKindDto.INTERVAL
            HubOccurrenceKind.Unknown -> HubOccurrenceKindDto.UNKNOWN
        },
        startUnixSeconds,
        endUnixSeconds,
    )

internal fun HubOccurrenceDto.toModel() =
    HubOccurrence(
        when (kind) {
            HubOccurrenceKindDto.KNOWN -> HubOccurrenceKind.Known
            HubOccurrenceKindDto.INTERVAL -> HubOccurrenceKind.Interval
            HubOccurrenceKindDto.UNKNOWN -> HubOccurrenceKind.Unknown
        },
        startUnixS,
        endUnixS,
    )

private fun HubSourceInput.toNative() =
    HubSourceInputDto(existingSourceId, issuerId, kind?.toNative(), code, provenance, certainty.toNative())

internal fun HubReceiptInput.toNative() =
    HubReceiptInputDto(
        expectedStockRevision,
        expectedCatalogRevision,
        variantId,
        supplierId,
        producerId,
        source.toNative(),
        wholePacks,
        receivingId,
        plannedLocationId,
        occurrence.toNative(),
        observation,
    )

internal fun HubStockQuery.toNative() =
    HubStockQueryDto(expectedCatalogRevision, expectedStockRevision, offset, limit, locationId, traceId, text)

private fun HubSourceDto.toModel() = HubSource(id, issuer?.toModel(), kind?.toModel(), code, provenance, certainty.toModel())

internal fun HubQuantityWorkDto.toModel() = HubQuantityWork(required, completed, noLongerRequired, remaining, revision, target?.toModel())

private fun HubStockPositionDto.toModel() = HubStockPosition(groupId, traceId, variantId, locationId, wholePacks, revision)

internal fun HubReceiptTraceDto.toModel() =
    HubReceiptTrace(
        id,
        variant.toModel(),
        product.toModel(),
        supplier?.toModel(),
        producer?.toModel(),
        source.toModel(),
        sourceNeedsReview,
        originalWholePacks,
        receiving.toModel(),
        plannedLocation?.toModel(),
        occurrence.toModel(),
        UnixSeconds(recordedAtUnixS),
        IdentityId.fromPublicKeyHex(actorPublicKey),
        observation,
    )

internal fun HubStockPageDto.toModel() =
    HubStockPage(
        HubHandlingBasis(handlingBasis.id, handlingBasis.version, handlingBasis.label, handlingBasis.siteTimezone),
        stockRevision,
        catalogRevision,
        offset,
        total,
        more,
        rows.map {
            HubStockRow(
                it.product.toModel(),
                it.position.toModel(),
                it.location.toModel(),
                it.trace.toModel(),
                it.putAway.toModel(),
            )
        },
    )

internal fun HubStockReceiptDto.toModel() =
    HubStockReceipt(OperationId.from(operationId), stockRevision, trace.toModel(), initialPosition.toModel(), initialPutAway.toModel())
