package org.harvestcircle.application

import kotlin.time.Instant

/** Unsubmitted, owner-scoped presentation input. Native commands decide consequences. */
data class CountFields(
    val quantity: String = "",
    val variantId: String? = null,
    val locationId: String? = null,
    val physicalLabel: String = "",
    val physicalObservation: String = "",
    val historyReference: String = "",
    val sourceCertainty: HubSourceCertainty = HubSourceCertainty.Unknown,
    val sourceKind: HubSourceKind = HubSourceKind.ProducerBatch,
    val issuerId: String? = null,
    val sourceCode: String = "",
    val sourceProvenance: String = "",
    val cutoffKind: HubOccurrenceKind = HubOccurrenceKind.Known,
    val cutoffStart: String = "",
    val cutoffEnd: String = "",
    val observation: String = "",
    val confirmed: Boolean = false,
) {
    internal fun bounded() =
        listOf(
            quantity,
            physicalLabel,
            physicalObservation,
            historyReference,
            sourceCode,
            sourceProvenance,
            cutoffStart,
            cutoffEnd,
            observation,
        ).all { it.length <= 1024 } &&
            (variantId?.length ?: 0) <= 32 &&
            (locationId?.length ?: 0) <= 32 &&
            (issuerId?.length ?: 0) <= 32

    internal fun cutoff() =
        when (cutoffKind) {
            HubOccurrenceKind.Unknown -> HubOccurrence(HubOccurrenceKind.Unknown)
            HubOccurrenceKind.Known -> HubOccurrence(HubOccurrenceKind.Known, Instant.parse(cutoffStart.trim()).epochSeconds)
            HubOccurrenceKind.Interval ->
                HubOccurrence(
                    HubOccurrenceKind.Interval,
                    Instant.parse(cutoffStart.trim()).epochSeconds,
                    Instant.parse(cutoffEnd.trim()).epochSeconds,
                )
        }

    internal fun identification() = HubPhysicalIdentification(physicalLabel, physicalObservation)

    internal fun source() =
        if (sourceCertainty == HubSourceCertainty.Unknown) {
            HubSourceInput()
        } else {
            HubSourceInput(
                issuerId = issuerId,
                kind = sourceKind,
                code = sourceCode,
                provenance = sourceProvenance,
                certainty = sourceCertainty,
            )
        }
}

sealed interface CountTarget {
    data object Opening : CountTarget

    data class Observe(
        val groupId: String,
    ) : CountTarget

    data class Review(
        val countId: String,
    ) : CountTarget
}

sealed interface CountContext {
    val stockRevision: ULong
    val catalogRevision: ULong
    val references: List<HubReferenceRecord>

    data class Opening(
        override val stockRevision: ULong,
        override val catalogRevision: ULong,
        override val references: List<HubReferenceRecord>,
    ) : CountContext

    data class Observe(
        val group: HubGroupView,
        override val references: List<HubReferenceRecord>,
    ) : CountContext {
        override val stockRevision get() = group.stockRevision
        override val catalogRevision get() = group.catalogRevision
    }

    data class Review(
        val review: HubCountReview,
        override val references: List<HubReferenceRecord>,
    ) : CountContext {
        override val stockRevision get() = review.stockRevision
        override val catalogRevision get() = review.catalogRevision
    }
}

data class CountDraft(
    val context: CountContext,
    val fields: CountFields,
)

sealed interface CountRequest {
    data class Opening(
        val input: HubOpeningInput,
    ) : CountRequest

    data class Observe(
        val input: HubCountObserveInput,
    ) : CountRequest

    data class Post(
        val input: HubCountPostInput,
    ) : CountRequest
}

internal fun CountDraft.request(): CountRequest =
    when (val current = context) {
        is CountContext.Opening ->
            CountRequest.Opening(
                HubOpeningInput(
                    current.stockRevision,
                    current.catalogRevision,
                    requireNotNull(
                        fields.variantId,
                    ),
                    requireNotNull(
                        fields.locationId,
                    ),
                    requireNotNull(fields.quantity.toUIntOrNull()),
                    fields.identification(),
                    fields.source(),
                    fields.historyReference.takeIf {
                        it.isNotBlank()
                    },
                    fields.cutoff(),
                    fields.observation,
                ),
            )
        is CountContext.Observe ->
            CountRequest.Observe(
                HubCountObserveInput(
                    current.stockRevision,
                    current.catalogRevision,
                    current.group.group.id,
                    current.group.group.revision,
                    requireNotNull(fields.quantity.toUIntOrNull()),
                    fields.cutoff(),
                    fields.observation,
                ),
            )
        is CountContext.Review ->
            CountRequest.Post(
                HubCountPostInput(
                    current.stockRevision,
                    current.catalogRevision,
                    current.review.observation.id,
                    fields.confirmed,
                    if (current.review.difference is HubCountDifference.Surplus) fields.identification() else null,
                    fields.observation,
                ),
            )
    }

data class CountAttempt(
    val operation: OperationId,
    val request: CountRequest,
)

sealed interface CountResult {
    val operation: OperationId

    data class Opened(
        val receipt: HubOpeningReceipt,
    ) : CountResult {
        override val operation get() = receipt.operationId
    }

    data class Observed(
        val observation: HubCountObservation,
    ) : CountResult {
        override val operation get() = observation.operationId
    }

    data class Posted(
        val receipt: HubCountPostReceipt,
    ) : CountResult {
        override val operation get() = receipt.operationId
    }
}

sealed interface CountSubmission {
    val attempt: CountAttempt

    data class Pending(
        override val attempt: CountAttempt,
    ) : CountSubmission

    data class Uncertain(
        override val attempt: CountAttempt,
        val message: String,
    ) : CountSubmission

    data class Rejected(
        override val attempt: CountAttempt,
        val problem: ApplicationProblem,
    ) : CountSubmission

    data class Recorded(
        override val attempt: CountAttempt,
        val result: CountResult,
    ) : CountSubmission
}

data class CountViewState(
    val available: Boolean = false,
    val open: Boolean = false,
    val loading: Boolean = false,
    val target: CountTarget? = null,
    val draft: CountDraft? = null,
    val needsReview: Boolean = false,
    val review: CountContext? = null,
    val submission: CountSubmission? = null,
    val message: String? = null,
)
