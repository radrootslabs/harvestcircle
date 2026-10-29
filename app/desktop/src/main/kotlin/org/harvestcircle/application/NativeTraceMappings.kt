package org.harvestcircle.application

import org.harvestcircle.ffi.HubLocalOutputDto
import org.harvestcircle.ffi.HubLocalOutputRequestDto
import org.harvestcircle.ffi.HubTraceHistoryKindDto
import org.harvestcircle.ffi.HubTraceHistoryPageDto
import org.harvestcircle.ffi.HubTraceHistoryQueryDto
import org.harvestcircle.ffi.HubTracePageDto
import org.harvestcircle.ffi.HubTraceQueryDto
import org.harvestcircle.ffi.HubTraceScopeDto
import org.harvestcircle.ffi.HubTraceSelectionDto
import org.harvestcircle.ffi.HubTraceSnapshotDto
import org.harvestcircle.ffi.HubTraceSnapshotQueryDto

internal fun HubTraceSelection.toNative() =
    HubTraceSelectionDto(
        when (scope) {
            HubTraceScope.All -> HubTraceScopeDto.ALL
            HubTraceScope.Unresolved -> HubTraceScopeDto.UNRESOLVED
            HubTraceScope.Receipt -> HubTraceScopeDto.RECEIPT
            HubTraceScope.SourceImpact -> HubTraceScopeDto.SOURCE_IMPACT
        },
        receiptId,
        sourceId,
        text,
    )

private fun HubTraceSelectionDto.toModel() =
    HubTraceSelection(
        when (scope) {
            HubTraceScopeDto.ALL -> HubTraceScope.All
            HubTraceScopeDto.UNRESOLVED -> HubTraceScope.Unresolved
            HubTraceScopeDto.RECEIPT -> HubTraceScope.Receipt
            HubTraceScopeDto.SOURCE_IMPACT -> HubTraceScope.SourceImpact
        },
        receiptId,
        sourceId,
        text,
    )

internal fun HubTraceQuery.toNative() =
    HubTraceQueryDto(selection.toNative(), expectedStockRevision, expectedCatalogRevision, offset, limit)

internal fun HubTraceSnapshotQuery.toNative() = HubTraceSnapshotQueryDto(traceId, expectedStockRevision, expectedCatalogRevision)

internal fun HubTraceHistoryQuery.toNative() =
    HubTraceHistoryQueryDto(traceId, expectedStockRevision, expectedCatalogRevision, offset, limit)

internal fun HubTracePageDto.toModel() =
    HubTracePage(
        stockRevision,
        catalogRevision,
        selection.toModel(),
        offset,
        total,
        more,
        rows.map { HubTraceRow(it.trace.toModel(), it.onHandWholePacks, it.departedWholePacks) },
    )

internal fun HubTraceSnapshotDto.toModel() =
    HubTraceSnapshot(
        hubId,
        stockRevision,
        catalogRevision,
        trace.toModel(),
        onHandWholePacks,
        departedWholePacks,
        onHand.map {
            HubTracePosition(
                it.groupId,
                it.groupRevision,
                it.wholePacks,
                it.location.toModel(),
                it.identification?.toModel(),
                it.acceptance.toModel(),
                it.acceptanceDecisionId,
                it.activeHoldIds,
            )
        },
        exits.map {
            HubTraceExit(
                it.groupId,
                it.wholePacks,
                it.departureId,
                OperationId.from(it.operationId),
                it.stockRevision,
                it.preparationId,
                it.identificationId,
                it.recipient.toModel(),
                IdentityId.fromPublicKeyHex(it.actorPublicKey),
                UnixSeconds(it.recordedAtUnixS),
                it.occurrence.toModel(),
                it.observation,
            )
        },
    )

private fun HubTraceHistoryKindDto.toModel() =
    when (this) {
        HubTraceHistoryKindDto.RECEIPT -> HubTraceHistoryKind.Receipt
        HubTraceHistoryKindDto.COUNTED_OPENING -> HubTraceHistoryKind.CountedOpening
        HubTraceHistoryKindDto.MOVE -> HubTraceHistoryKind.Move
        HubTraceHistoryKindDto.SPLIT -> HubTraceHistoryKind.Split
        HubTraceHistoryKindDto.ACCEPTANCE -> HubTraceHistoryKind.Acceptance
        HubTraceHistoryKindDto.PLACE_HOLD -> HubTraceHistoryKind.PlaceHold
        HubTraceHistoryKindDto.RELEASE_HOLD -> HubTraceHistoryKind.ReleaseHold
        HubTraceHistoryKindDto.REPLAN -> HubTraceHistoryKind.Replan
        HubTraceHistoryKindDto.NEW_WORK -> HubTraceHistoryKind.NewWork
        HubTraceHistoryKindDto.OPEN_DISCREPANCY -> HubTraceHistoryKind.OpenDiscrepancy
        HubTraceHistoryKindDto.RESOLVE_DISCREPANCY -> HubTraceHistoryKind.ResolveDiscrepancy
        HubTraceHistoryKindDto.PREPARE_IDENTIFICATION -> HubTraceHistoryKind.PrepareIdentification
        HubTraceHistoryKindDto.REVISE_IDENTIFICATION -> HubTraceHistoryKind.ReviseIdentification
        HubTraceHistoryKindDto.ISSUE_IDENTIFICATION -> HubTraceHistoryKind.IssueIdentification
        HubTraceHistoryKindDto.REPLACE_IDENTIFICATION -> HubTraceHistoryKind.ReplaceIdentification
        HubTraceHistoryKindDto.MARK_IDENTIFICATION_UNUSED -> HubTraceHistoryKind.MarkIdentificationUnused
        HubTraceHistoryKindDto.DEPARTURE -> HubTraceHistoryKind.Departure
    }

internal fun HubTraceHistoryPageDto.toModel() =
    HubTraceHistoryPage(
        traceId,
        stockRevision,
        catalogRevision,
        offset,
        total,
        more,
        rows.map {
            HubTraceHistoryEntry(
                OperationId.from(it.operationId),
                it.stockRevision,
                it.kind.toModel(),
                IdentityId.fromPublicKeyHex(it.actorPublicKey),
                UnixSeconds(it.recordedAtUnixS),
                it.occurrence?.toModel(),
                it.observation,
                it.resultId,
            )
        },
    )

internal fun HubLocalOutputRequest.toNative() =
    when (this) {
        is HubLocalOutputRequest.Identification -> HubLocalOutputRequestDto.Identification(preparationId, identificationId)
        is HubLocalOutputRequest.Trace -> HubLocalOutputRequestDto.Trace(traceId, expectedStockRevision, expectedCatalogRevision)
    }

internal fun HubLocalOutputDto.toModel() = HubLocalOutput(identity, suggestedFilename, html)
