package org.harvestcircle.application

import kotlin.time.Instant

/** Unsubmitted input only. A selected record and a preview never grant an effect. */
data class ActualFields(
    val quantity: String = "",
    val variantId: String? = null,
    val recipientId: String? = null,
    val sourceKind: HubActualSourceKind = HubActualSourceKind.Unknown,
    val sourceTraceIds: List<String> = emptyList(),
    val occurrenceKind: HubOccurrenceKind = HubOccurrenceKind.Unknown,
    val occurrenceStart: String = "",
    val occurrenceEnd: String = "",
    val observation: String = "",
    val groupId: String? = null,
    val traceId: String? = null,
    val source: ActualSourceChoice? = null,
    val known: ActualKnownChoice? = null,
    val sourceEvidence: String = "",
    val quantityEvidence: String = "",
    val recipientEvidence: String = "",
    val timingEvidence: String = "",
    val duplicateBasis: String = "",
    val confirmed: Boolean = false,
) {
    internal fun bounded() =
        listOf(
            quantity,
            occurrenceStart,
            occurrenceEnd,
            observation,
            sourceEvidence,
            quantityEvidence,
            recipientEvidence,
            timingEvidence,
            duplicateBasis,
        ).all { it.length <= 1024 } &&
            sourceTraceIds.size <= 64 &&
            sourceTraceIds.all { it.length <= 32 } &&
            listOf(variantId, recipientId, groupId, traceId).all { (it?.length ?: 0) <= 32 }

    internal fun occurrence() =
        when (occurrenceKind) {
            HubOccurrenceKind.Unknown -> HubOccurrence(HubOccurrenceKind.Unknown)
            HubOccurrenceKind.Known -> HubOccurrence(HubOccurrenceKind.Known, Instant.parse(occurrenceStart.trim()).epochSeconds)
            HubOccurrenceKind.Interval ->
                HubOccurrence(
                    HubOccurrenceKind.Interval,
                    Instant.parse(occurrenceStart.trim()).epochSeconds,
                    Instant.parse(occurrenceEnd.trim()).epochSeconds,
                )
        }

    internal fun quantityOrUnknown(): UInt? =
        quantity
            .trim()
            .takeIf { it.isNotEmpty() }
            ?.toUInt()
            ?.also { require(it > 0U) }

    companion object {
        fun from(report: HubActualReport): ActualFields {
            val original = report.reported
            return ActualFields(
                quantity = original.wholePacks?.toString().orEmpty(),
                recipientId = original.recipientId,
                occurrenceKind = original.occurrence.kind,
                occurrenceStart =
                    original.occurrence.startUnixSeconds
                        ?.let { Instant.fromEpochSeconds(it).toString() }
                        .orEmpty(),
                occurrenceEnd =
                    original.occurrence.endUnixSeconds
                        ?.let { Instant.fromEpochSeconds(it).toString() }
                        .orEmpty(),
            )
        }
    }
}

sealed interface ActualTarget {
    data object Report : ActualTarget

    data class Departure(
        val report: HubActualReport,
    ) : ActualTarget

    data class Link(
        val report: HubActualReport,
    ) : ActualTarget
}

data class ActualSourceChoice(
    val operation: OperationId,
    val groupId: String,
    val label: String,
) {
    val key get() = "${operation.value}:$groupId"
}

data class ActualKnownChoice(
    val operation: OperationId,
    val traceId: String,
    val variantId: String,
    val label: String,
) {
    val key get() = "${operation.value}:$traceId:$variantId"
}

data class ActualContext(
    val references: List<HubReferenceRecord> = emptyList(),
    val stock: HubStockPage? = null,
    val traces: HubTracePage? = null,
    val group: HubGroupView? = null,
    val treatment: HubGroupTreatment? = null,
    val sources: List<ActualSourceChoice> = emptyList(),
    val sourceHistory: HubTraceHistoryPage? = null,
    val lineage: HubLineagePage? = null,
    val known: List<ActualKnownChoice> = emptyList(),
)

data class ActualDraft(
    val target: ActualTarget,
    val context: ActualContext = ActualContext(),
    val fields: ActualFields = ActualFields(),
) {
    internal fun request(): ActualRequest =
        when (val selected = target) {
            ActualTarget.Report -> {
                require(fields.observation.isNotBlank())
                val claims =
                    if (fields.sourceKind ==
                        HubActualSourceKind.Unknown
                    ) {
                        emptyList()
                    } else {
                        fields.sourceTraceIds.distinct().map { HubActualSourceClaim(it) }
                    }
                require(fields.sourceKind != HubActualSourceKind.Possible || claims.isNotEmpty())
                require(fields.sourceKind != HubActualSourceKind.Reported || claims.size == 1)
                ActualRequest.Report(
                    HubActualReportInput(
                        fields.variantId,
                        fields.recipientId,
                        fields.quantityOrUnknown(),
                        fields.sourceKind,
                        claims,
                        fields.occurrence(),
                        fields.observation,
                    ),
                )
            }
            is ActualTarget.Departure -> {
                val current = requireNotNull(context.group)
                require(current.group.id == fields.groupId)
                val source = requireNotNull(fields.source)
                val packs = requireNotNull(fields.quantityOrUnknown())
                require(
                    listOf(
                        fields.sourceEvidence,
                        fields.quantityEvidence,
                        fields.recipientEvidence,
                        fields.timingEvidence,
                        fields.duplicateBasis,
                    ).all {
                        it.isNotBlank()
                    },
                )
                ActualRequest.Departure(
                    HubActualDepartureInput(
                        selected.report.id,
                        current.stockRevision,
                        current.catalogRevision,
                        current.group.id,
                        current.group.revision,
                        packs,
                        requireNotNull(fields.recipientId),
                        fields.occurrence(),
                        source.operation,
                        source.groupId,
                        fields.sourceEvidence,
                        fields.quantityEvidence,
                        fields.recipientEvidence,
                        fields.timingEvidence,
                        fields.duplicateBasis,
                    ),
                )
            }
            is ActualTarget.Link -> {
                val known = requireNotNull(fields.known)
                require(fields.duplicateBasis.isNotBlank())
                ActualRequest.Link(
                    HubActualLinkInput(selected.report.id, known.operation, known.traceId, known.variantId, fields.duplicateBasis),
                )
            }
        }
}

sealed interface ActualRequest {
    data class Report(
        val input: HubActualReportInput,
    ) : ActualRequest

    data class Departure(
        val input: HubActualDepartureInput,
    ) : ActualRequest

    data class Link(
        val input: HubActualLinkInput,
    ) : ActualRequest
}

data class ActualAttempt(
    val operation: OperationId,
    val request: ActualRequest,
)

sealed interface ActualPreview {
    val attempt: ActualAttempt

    data class Departure(
        override val attempt: ActualAttempt,
        val value: HubActualDeparturePreview,
    ) : ActualPreview

    data class Link(
        override val attempt: ActualAttempt,
        val value: HubActualLinkPreview,
    ) : ActualPreview
}

sealed interface ActualResult {
    val operation: OperationId

    data class Reported(
        val value: HubActualReport,
    ) : ActualResult {
        override val operation get() = value.operationId
    }

    data class Reviewed(
        val value: HubActualReviewReceipt,
    ) : ActualResult {
        override val operation get() = value.operationId
    }
}

sealed interface ActualSubmission {
    val attempt: ActualAttempt

    data class Pending(
        override val attempt: ActualAttempt,
    ) : ActualSubmission

    data class Recorded(
        override val attempt: ActualAttempt,
        val result: ActualResult,
    ) : ActualSubmission

    data class Rejected(
        override val attempt: ActualAttempt,
        val problem: ApplicationProblem,
    ) : ActualSubmission

    data class Uncertain(
        override val attempt: ActualAttempt,
        val explanation: String,
    ) : ActualSubmission
}

enum class ActualLookupKind { Report, Review }

data class ActualOriginalLookup(
    val kind: ActualLookupKind = ActualLookupKind.Report,
    val operationText: String = "",
    val result: ActualResult? = null,
)

data class ActualViewState(
    val available: Boolean = false,
    val open: Boolean = false,
    val loading: Boolean = false,
    val filter: HubActualReportFilter = HubActualReportFilter.All,
    val page: HubActualReportPage? = null,
    val draft: ActualDraft? = null,
    val currentContext: ActualContext? = null,
    val needsReview: Boolean = false,
    val preview: ActualPreview? = null,
    val originalLookup: ActualOriginalLookup? = null,
    val submission: ActualSubmission? = null,
    val message: String? = null,
)
