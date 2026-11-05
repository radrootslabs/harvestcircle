package org.harvestcircle.application

import org.harvestcircle.ffi.HubActualClockAssessmentDto
import org.harvestcircle.ffi.HubActualDepartureInputDto
import org.harvestcircle.ffi.HubActualDeparturePreviewDto
import org.harvestcircle.ffi.HubActualDuplicateDto
import org.harvestcircle.ffi.HubActualHoldAssessmentDto
import org.harvestcircle.ffi.HubActualHoldTimingDto
import org.harvestcircle.ffi.HubActualHoldUncertaintyDto
import org.harvestcircle.ffi.HubActualKnownDepartureDto
import org.harvestcircle.ffi.HubActualKnownInterpretationDto
import org.harvestcircle.ffi.HubActualLinkInputDto
import org.harvestcircle.ffi.HubActualLinkPreviewDto
import org.harvestcircle.ffi.HubActualReportDto
import org.harvestcircle.ffi.HubActualReportFilterDto
import org.harvestcircle.ffi.HubActualReportInputDto
import org.harvestcircle.ffi.HubActualReportPageDto
import org.harvestcircle.ffi.HubActualReportQueryDto
import org.harvestcircle.ffi.HubActualReportResolutionKindDto
import org.harvestcircle.ffi.HubActualReviewOriginalDto
import org.harvestcircle.ffi.HubActualReviewReceiptDto
import org.harvestcircle.ffi.HubActualSourceClaimDto
import org.harvestcircle.ffi.HubActualSourceKindDto

internal fun HubActualReportInput.toNative() =
    HubActualReportInputDto(
        variantId,
        recipientId,
        wholePacks,
        when (sourceKind) {
            HubActualSourceKind.Unknown -> HubActualSourceKindDto.UNKNOWN
            HubActualSourceKind.Possible -> HubActualSourceKindDto.POSSIBLE
            HubActualSourceKind.Reported -> HubActualSourceKindDto.ESTABLISHED_CLAIM
        },
        sourceClaims.map { HubActualSourceClaimDto(it.traceId, it.groupId, it.locationId) },
        occurrence.toNative(),
        observation,
    )

internal fun HubActualReportInputDto.toModel(): HubActualReportInput {
    // Reject inconsistent wire layouts rather than silently dropping claims.
    require(sourceClaims.size <= 64)
    val source =
        when (sourceKind) {
            HubActualSourceKindDto.UNKNOWN -> {
                require(sourceClaims.isEmpty())
                HubActualSourceKind.Unknown
            }
            HubActualSourceKindDto.POSSIBLE -> {
                require(sourceClaims.isNotEmpty())
                HubActualSourceKind.Possible
            }
            HubActualSourceKindDto.ESTABLISHED_CLAIM -> {
                require(sourceClaims.size == 1)
                HubActualSourceKind.Reported
            }
        }
    return HubActualReportInput(
        variantId,
        recipientId,
        wholePacks,
        source,
        sourceClaims.map {
            HubActualSourceClaim(it.traceId, it.groupId, it.locationId)
        },
        occurrence.toModel(),
        observation,
    )
}

internal fun HubActualReportDto.toModel() =
    HubActualReport(
        id,
        OperationId.from(operationId),
        hubId,
        stockRevision,
        catalogRevision,
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        when (clockAssessment) {
            HubActualClockAssessmentDto.UNKNOWN -> HubActualClockAssessment.Unknown
            HubActualClockAssessmentDto.NOT_AFTER_RECORDED_TIME -> HubActualClockAssessment.NotAfterRecordedTime
            HubActualClockAssessmentDto.RECORDING_CLOCK_CONFLICT -> HubActualClockAssessment.RecordingClockConflict
        },
        reported.toModel(),
    )

internal fun HubActualReportQuery.toNative() =
    HubActualReportQueryDto(
        expectedStockRevision,
        expectedCatalogRevision,
        offset,
        limit,
        when (filter) {
            HubActualReportFilter.Unresolved -> HubActualReportFilterDto.UNRESOLVED
            HubActualReportFilter.All -> HubActualReportFilterDto.ALL
        },
    )

internal fun HubActualReportPageDto.toModel() =
    HubActualReportPage(
        stockRevision,
        catalogRevision,
        offset,
        total,
        more,
        rows.map {
            HubActualReportRow(
                it.report.toModel(),
                it.reviewOperationId?.let(OperationId::from),
                when (it.resolutionKind) {
                    null -> null
                    HubActualReportResolutionKindDto.ACTUAL_REVIEW -> HubActualReportResolutionKind.ActualReview
                    HubActualReportResolutionKindDto.ADDITIONAL_QUANTITY -> HubActualReportResolutionKind.AdditionalQuantity
                },
            )
        },
    )

internal fun HubActualDepartureInput.toNative() =
    HubActualDepartureInputDto(
        reportId,
        expectedStockRevision,
        expectedCatalogRevision,
        groupId,
        expectedGroupRevision,
        wholePacks,
        recipientId,
        occurrence.toNative(),
        sourceOperationId.value,
        sourceGroupId,
        sourceEvidence,
        quantityEvidence,
        recipientEvidence,
        timingEvidence,
        duplicateReviewBasis,
    )

internal fun HubActualLinkInput.toNative() =
    HubActualLinkInputDto(reportId, knownOperationId.value, knownTraceId, knownVariantId, verification)

private fun HubActualHoldTimingDto.toModel() =
    when (this) {
        HubActualHoldTimingDto.ESTABLISHED_BREACH -> HubActualHoldTiming.EstablishedBreach
        HubActualHoldTimingDto.BEFORE_HOLD -> HubActualHoldTiming.BeforeHold
        HubActualHoldTimingDto.AFTER_RELEASE -> HubActualHoldTiming.AfterRelease
        HubActualHoldTimingDto.TIMING_UNKNOWN -> HubActualHoldTiming.TimingUnknown
    }

private fun HubActualHoldUncertaintyDto.toModel() =
    when (this) {
        HubActualHoldUncertaintyDto.SCOPE_NOT_ESTABLISHED -> HubActualHoldUncertainty.ScopeNotEstablished
        HubActualHoldUncertaintyDto.MISSING_BIRTH -> HubActualHoldUncertainty.MissingBirth
        HubActualHoldUncertaintyDto.UNKNOWN_BIRTH_TIME -> HubActualHoldUncertainty.UnknownBirthTime
        HubActualHoldUncertaintyDto.GROUP_NOT_PRESENT_AT_DEPARTURE -> HubActualHoldUncertainty.GroupNotPresentAtDeparture
        HubActualHoldUncertaintyDto.ANCHOR_NOT_PRESENT_AT_EFFECTIVE_TIME -> HubActualHoldUncertainty.AnchorNotPresentAtEffectiveTime
        HubActualHoldUncertaintyDto.UNCERTAIN_PHYSICAL_ANCESTRY -> HubActualHoldUncertainty.UncertainPhysicalAncestry
        HubActualHoldUncertaintyDto.EXPANDED_PHYSICAL_SCOPE -> HubActualHoldUncertainty.ExpandedPhysicalScope
        HubActualHoldUncertaintyDto.ANCHOR_COHORT_UNVERIFIED -> HubActualHoldUncertainty.AnchorCohortUnverified
    }

internal fun HubActualHoldAssessmentDto.toModel(): HubActualHoldAssessment =
    when (this) {
        is HubActualHoldAssessmentDto.Unresolved ->
            HubActualHoldAssessment.Unresolved(
                original.toModel(),
                conditionalTiming.toModel(),
                reason.toModel(),
            )
        is HubActualHoldAssessmentDto.Verified ->
            HubActualHoldAssessment.Verified(
                original.toModel(),
                groupId,
                occurrence.toModel(),
                lineageRevision,
                timing.toModel(),
            )
    }

private fun HubActualKnownDepartureDto.toModel() =
    HubActualKnownDeparture(
        OperationId.from(operationId),
        IdentityId.fromPublicKeyHex(actorPublicKey),
        hubId,
        stockRevision,
        catalogRevision,
        traceId,
        variant.toModel(),
        recipient.toModel(),
        occurrence.toModel(),
        UnixSeconds(recordedAtUnixS),
        wholePacks,
        lines.map { HubActualKnownLine(it.groupId, it.location.toModel(), it.wholePacks) },
    )

internal fun HubActualDuplicateDto.toModel() =
    HubActualDuplicate(
        OperationId.from(operationId),
        report.toModel(),
        known.toModel(),
        IdentityId.fromPublicKeyHex(actorPublicKey),
        UnixSeconds(recordedAtUnixS),
        verification,
        interpretation?.toModel(),
    )

internal fun HubActualDeparturePreviewDto.toModel() =
    HubActualDeparturePreview(
        report.toModel(),
        stockRevision,
        catalogRevision,
        proposed.toModel(),
        historicalHolds.map {
            it.toModel()
        },
    )

internal fun HubActualLinkPreviewDto.toModel() = HubActualLinkPreview(stockRevision, catalogRevision, proposed.toModel())

internal fun HubActualReviewReceiptDto.toModel() =
    HubActualReviewReceipt(
        OperationId.from(operationId),
        stockRevision,
        catalogRevision,
        when (val result = original) {
            is HubActualReviewOriginalDto.Departure ->
                HubActualReviewOriginal
                    .Departure(
                        result.report.toModel(),
                        result.original.toModel(),
                        result.historicalHolds.map {
                            it.toModel()
                        },
                    )
            is HubActualReviewOriginalDto.Duplicate -> HubActualReviewOriginal.Duplicate(result.original.toModel())
        },
    )

internal fun HubActualKnownInterpretationDto.toModel() =
    HubActualKnownInterpretation(
        stockRevision,
        catalogRevision,
        UnixSeconds(latestRecordedAtUnixS),
        originalWholePacks,
        effectiveWholePacks,
        recipient?.toModel(),
        recipientHistory.map { correction -> correction.toModel() },
        quantities.map { quantity -> quantity.toModel() },
    )
