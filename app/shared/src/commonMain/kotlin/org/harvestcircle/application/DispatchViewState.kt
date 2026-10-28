package org.harvestcircle.application

/** Retained client input. Native commands own material, versions and effects. */
data class DispatchDraft(
    val kind: HubDispatchKind,
    val versions: HandlingVersions,
    val target: HubDispatchRecord? = null,
    val recipientId: String? = null,
    val lines: List<HubDispatchLine> = emptyList(),
    val observation: String = "",
) {
    fun input(): HubDispatchInput {
        fun material() =
            HubDispatchMaterialInput(
                versions.stock,
                versions.catalog,
                requireNotNull(recipientId),
                lines.map { HubDispatchLineInput(it.groupId, it.groupRevision, it.wholePacks) },
                observation,
            )

        fun action(): HubDispatchActionInput {
            val record = requireNotNull(target)
            return HubDispatchActionInput(versions.stock, versions.catalog, record.id, record.revision, observation)
        }
        return when (kind) {
            HubDispatchKind.Prepare -> HubDispatchInput.Prepare(material())
            HubDispatchKind.Revise -> HubDispatchInput.Revise(HubDispatchEditInput(requireNotNull(target).id, target.revision, material()))
            HubDispatchKind.Replace ->
                HubDispatchInput.Replace(
                    HubDispatchEditInput(requireNotNull(target).id, target.revision, material()),
                )
            HubDispatchKind.Issue -> HubDispatchInput.Issue(action())
            HubDispatchKind.MarkUnused -> HubDispatchInput.MarkUnused(action())
        }
    }
}

data class DispatchAttempt(
    val operation: OperationId,
    val input: HubDispatchInput,
)

sealed interface DispatchSubmission {
    val attempt: DispatchAttempt

    data class Pending(
        override val attempt: DispatchAttempt,
    ) : DispatchSubmission

    data class Recorded(
        override val attempt: DispatchAttempt,
        val receipt: HubDispatchReceipt,
    ) : DispatchSubmission

    data class Rejected(
        override val attempt: DispatchAttempt,
        val problem: ApplicationProblem,
    ) : DispatchSubmission

    data class Uncertain(
        override val attempt: DispatchAttempt,
        val message: String,
    ) : DispatchSubmission
}

data class DispatchContextReview(
    val draft: DispatchDraft,
    val references: List<HubReferenceRecord>,
    val stock: HubStockPage,
)

data class DispatchViewState(
    val available: Boolean = false,
    val loading: Boolean = false,
    val queryText: String = "",
    val filter: HubDispatchFilter = HubDispatchFilter.All,
    val page: HubDispatchPage? = null,
    val selected: HubDispatchView? = null,
    val selectedVersionId: String? = null,
    val stock: HubStockPage? = null,
    val references: List<HubReferenceRecord> = emptyList(),
    val draft: DispatchDraft? = null,
    val formOpen: Boolean = false,
    val needsReview: Boolean = false,
    val review: DispatchContextReview? = null,
    val submission: DispatchSubmission? = null,
    val message: String? = null,
)

internal fun HubGroupView.dispatchLine() =
    HubDispatchLine(
        group.id,
        group.revision,
        group.wholePacks,
        group.identification,
        location,
        product,
        variant,
        trace,
    )

internal fun HubDispatchRecord.material() = draft ?: versions.lastOrNull()?.material
