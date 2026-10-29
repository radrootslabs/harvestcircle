package org.harvestcircle.application

/** Presentation input; the selected issued paper and native request never retarget. */
data class DepartureDraft(
    val paper: HubDispatchRecord,
    val request: HubDepartureReviewInput,
    val context: HubDepartureReview? = null,
    val fields: HandlingFields = HandlingFields(),
    val confirmed: Boolean = false,
)

data class DepartureAttempt(
    val operation: OperationId,
    val input: HubDepartureInput,
)

sealed interface DepartureSubmission {
    val attempt: DepartureAttempt

    data class Pending(
        override val attempt: DepartureAttempt,
    ) : DepartureSubmission

    data class Recorded(
        override val attempt: DepartureAttempt,
        val receipt: HubDepartureReceipt,
    ) : DepartureSubmission

    data class Rejected(
        override val attempt: DepartureAttempt,
        val problem: ApplicationProblem,
    ) : DepartureSubmission

    data class Uncertain(
        override val attempt: DepartureAttempt,
        val message: String,
    ) : DepartureSubmission
}

data class DepartureViewState(
    val available: Boolean = false,
    val loading: Boolean = false,
    val formOpen: Boolean = false,
    val draft: DepartureDraft? = null,
    val review: HubDepartureReview? = null,
    val needsReview: Boolean = false,
    val submission: DepartureSubmission? = null,
    val workOffset: Int = 0,
    val message: String? = null,
)
