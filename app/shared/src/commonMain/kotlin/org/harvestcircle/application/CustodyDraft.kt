package org.harvestcircle.application

/** Unsubmitted presentation fields. Native admission resolves every reference and association. */
data class CustodyFields(
    val common: CountFields = CountFields(cutoffKind = HubOccurrenceKind.Unknown),
    val returnedById: String? = null,
    val plannedLocationId: String? = null,
    val departureOperation: String = "",
    val departureId: String = "",
    val originalGroupId: String = "",
    val dispositionKind: HubDispositionKind = HubDispositionKind.Destroyed,
) {
    internal fun bounded() =
        common.bounded() &&
            listOf(returnedById, plannedLocationId, departureId, originalGroupId).all { (it?.length ?: 0) <= 32 } &&
            departureOperation.length <= 36
}

sealed interface CustodyTarget {
    data object Return : CustodyTarget

    data class Dispose(
        val groupId: String,
    ) : CustodyTarget
}

data class CustodyContext(
    val stockRevision: ULong,
    val catalogRevision: ULong,
    val references: List<HubReferenceRecord>,
    val group: HubGroupView? = null,
)

data class CustodyDraft(
    val context: CustodyContext,
    val fields: CustodyFields = CustodyFields(),
    val departure: HubDepartureReceipt? = null,
)

sealed interface CustodyRequest {
    data class Return(
        val input: HubReturnInput,
    ) : CustodyRequest

    data class Dispose(
        val input: HubDispositionInput,
    ) : CustodyRequest
}

internal fun CustodyDraft.request(target: CustodyTarget): CustodyRequest {
    val common = fields.common
    return when (target) {
        CustodyTarget.Return -> {
            val packs = common.quantity.trim().toUIntOrNull()
            require(packs != null && packs > 0U)
            val claim = fields.departureId.trim().takeIf { it.isNotEmpty() }
            require(claim != null || fields.originalGroupId.isBlank())
            CustodyRequest.Return(
                HubReturnInput(
                    context.stockRevision,
                    context.catalogRevision,
                    requireNotNull(common.variantId),
                    requireNotNull(common.locationId),
                    fields.returnedById,
                    common.source(),
                    packs,
                    common.identification(),
                    fields.plannedLocationId,
                    claim?.let { HubReturnClaim(it, fields.originalGroupId.trim().takeIf(String::isNotEmpty)) },
                    common.cutoff(),
                    common.observation,
                ),
            )
        }
        is CustodyTarget.Dispose -> {
            val view = requireNotNull(context.group)
            require(view.group.id == target.groupId)
            CustodyRequest.Dispose(
                HubDispositionInput(
                    context.stockRevision,
                    context.catalogRevision,
                    view.group.id,
                    view.group.revision,
                    view.group.wholePacks,
                    fields.dispositionKind,
                    common.cutoff(),
                    common.observation,
                ),
            )
        }
    }
}

sealed interface CustodyResult {
    val operation: OperationId

    data class Returned(
        val receipt: HubReturnReceipt,
    ) : CustodyResult {
        override val operation get() = receipt.operationId
    }

    data class Disposed(
        val receipt: HubDispositionReceipt,
    ) : CustodyResult {
        override val operation get() = receipt.operationId
    }
}

data class CustodyAttempt(
    val operation: OperationId,
    val request: CustodyRequest,
)

sealed interface CustodySubmission {
    val attempt: CustodyAttempt

    data class Pending(
        override val attempt: CustodyAttempt,
    ) : CustodySubmission

    data class Uncertain(
        override val attempt: CustodyAttempt,
        val message: String,
    ) : CustodySubmission

    data class Rejected(
        override val attempt: CustodyAttempt,
        val problem: ApplicationProblem,
    ) : CustodySubmission

    data class Recorded(
        override val attempt: CustodyAttempt,
        val result: CustodyResult,
    ) : CustodySubmission
}

enum class CustodyLookupKind { Return, Disposition }

data class CustodyOriginalLookup(
    val kind: CustodyLookupKind = CustodyLookupKind.Return,
    val operationText: String = "",
    val result: CustodyResult? = null,
)

data class CustodyViewState(
    val available: Boolean = false,
    val open: Boolean = false,
    val loading: Boolean = false,
    val target: CustodyTarget? = null,
    val draft: CustodyDraft? = null,
    val review: CustodyContext? = null,
    val needsReview: Boolean = false,
    val submission: CustodySubmission? = null,
    val originalLookup: CustodyOriginalLookup? = null,
    val message: String? = null,
)
