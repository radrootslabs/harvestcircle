package org.harvestcircle.application

enum class LocalOutputAction { SaveHtml, Print }

sealed interface LocalOutputResult {
    data class Saved(
        val path: String,
    ) : LocalOutputResult

    data object PrintSubmitted : LocalOutputResult

    data object Cancelled : LocalOutputResult

    data object Failed : LocalOutputResult

    data object Uncertain : LocalOutputResult
}

/** Local I/O only. Implementations own and cancel only their own dialogs and jobs. */
interface LocalOutputPort {
    suspend fun perform(
        action: LocalOutputAction,
        document: HubLocalOutput,
    ): LocalOutputResult

    fun cancel()

    fun close()
}

data class LocalOutputState(
    val available: Boolean = false,
    val open: Boolean = false,
    val loading: Boolean = false,
    val busy: Boolean = false,
    val document: HubLocalOutput? = null,
    val title: String = "Local output",
    val message: String? = null,
    val printNeedsReview: Boolean = false,
)
