package org.harvestcircle.application

enum class HandlingTab { Work, Discrepancies }

data class HandlingAttempt(
    val operation: OperationId,
    val input: HubHandlingInput,
)

sealed interface HandlingSubmission {
    val attempt: HandlingAttempt

    data class Pending(
        override val attempt: HandlingAttempt,
    ) : HandlingSubmission

    data class Recorded(
        override val attempt: HandlingAttempt,
        val receipt: HubHandlingReceipt,
    ) : HandlingSubmission

    data class Rejected(
        override val attempt: HandlingAttempt,
        val problem: ApplicationProblem,
    ) : HandlingSubmission

    data class Uncertain(
        override val attempt: HandlingAttempt,
        val message: String,
    ) : HandlingSubmission
}

data class HandlingContextReview(
    val draft: HandlingDraft,
    val references: List<HubReferenceRecord>,
)

data class HandlingViewState(
    val available: Boolean = false,
    val loading: Boolean = false,
    val tab: HandlingTab = HandlingTab.Work,
    val queryText: String = "",
    val openOnly: Boolean = true,
    val work: HubWorkPage? = null,
    val discrepancies: HubDiscrepancyPage? = null,
    val references: List<HubReferenceRecord> = emptyList(),
    val group: HubGroupView? = null,
    val lineage: HubLineagePage? = null,
    val selectedWork: HubWorkRow? = null,
    val selectedDiscrepancy: HubDiscrepancyRow? = null,
    val draft: HandlingDraft? = null,
    val formOpen: Boolean = false,
    val submission: HandlingSubmission? = null,
    val needsReview: Boolean = false,
    val review: HandlingContextReview? = null,
    val message: String? = null,
)
