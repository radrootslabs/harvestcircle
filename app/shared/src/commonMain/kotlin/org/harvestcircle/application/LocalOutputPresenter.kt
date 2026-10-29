package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** Freezes one native rendering; local retries never reread or submit stock commands. */
class LocalOutputPresenter(
    private val runtime: HarvestCircleTraceRuntime?,
    private val port: LocalOutputPort?,
    private val scope: CoroutineScope,
) {
    private val mutableState = MutableStateFlow(LocalOutputState())
    val state = mutableState.asStateFlow()
    private var hub: String? = null
    private var closed = false
    private var generation = 0L
    private var job: Job? = null

    fun attach(id: String) {
        if (closed || hub == id) return
        invalidate()
        hub = id
        mutableState.value = LocalOutputState(available = runtime != null && port != null)
    }

    fun prepare(request: HubLocalOutputRequest) {
        val current = mutableState.value
        if (closed || !current.available || current.busy) return
        invalidate()
        val token = generation
        mutableState.value =
            LocalOutputState(
                available = true,
                open = true,
                loading = true,
                title =
                    when (request) {
                        is HubLocalOutputRequest.Identification -> "As-issued identification"
                        is HubLocalOutputRequest.Trace -> "Recorded local trace snapshot"
                    },
            )
        job =
            scope.launch {
                try {
                    val document = requireNotNull(runtime).localOutput(request)
                    if (isCurrent(token)) mutableState.value = mutableState.value.copy(loading = false, document = document)
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (isCurrent(token)) {
                        mutableState.value =
                            mutableState.value.copy(
                                loading = false,
                                message =
                                    (error as? ApplicationFailure)?.problem?.safeMessage
                                        ?: "This document could not be prepared. Return to its record and review the current context.",
                            )
                    }
                }
            }
    }

    fun perform(action: LocalOutputAction) {
        val current = mutableState.value
        val document = current.document ?: return
        if (closed || !current.open || current.busy || (action == LocalOutputAction.Print && current.printNeedsReview)) return
        val token = generation
        mutableState.value = current.copy(busy = true, message = null)
        job =
            scope.launch {
                val result =
                    try {
                        requireNotNull(port).perform(action, document)
                    } catch (error: CancellationException) {
                        throw error
                    } catch (_: Exception) {
                        LocalOutputResult.Uncertain
                    }
                if (isCurrent(token)) {
                    val currentState = mutableState.value
                    val printNeedsReview =
                        when (action) {
                            LocalOutputAction.Print -> result == LocalOutputResult.Uncertain
                            LocalOutputAction.SaveHtml -> currentState.printNeedsReview
                        }
                    mutableState.value =
                        currentState.copy(
                            busy = false,
                            printNeedsReview = printNeedsReview,
                            message =
                                when (result) {
                                    is LocalOutputResult.Saved -> "Saved locally: ${result.path}"
                                    LocalOutputResult.PrintSubmitted ->
                                        "Submitted to the print system. " +
                                            "Physical printing and recipient acknowledgement are not confirmed."
                                    LocalOutputResult.Cancelled ->
                                        "Cancelled before output submission. This same captured document remains available."
                                    LocalOutputResult.Failed ->
                                        "Local output failed before submission. This same captured document remains available."
                                    LocalOutputResult.Uncertain ->
                                        "Output may have reached the selected file or print system. " +
                                            "Check the destination or print queue before retrying. This captured document is unchanged."
                                },
                        )
                }
            }
    }

    fun acknowledgePrintReview() {
        if (!mutableState.value.busy) mutableState.value = mutableState.value.copy(printNeedsReview = false)
    }

    fun cancel() {
        if (!mutableState.value.busy) return
        port?.cancel()
        mutableState.value = mutableState.value.copy(message = "Cancellation requested for this output. Waiting for its local result…")
    }

    /** A pending local output must settle before ordinary navigation can hide it. */
    fun dismiss(): Boolean {
        if (!mutableState.value.open) return false
        if (mutableState.value.busy) {
            cancel()
        } else {
            generation += 1
            job?.cancel()
            mutableState.value = mutableState.value.copy(open = false, loading = false)
        }
        return true
    }

    private fun invalidate() {
        generation += 1
        port?.cancel()
        job?.cancel()
    }

    private fun isCurrent(token: Long) = !closed && token == generation

    fun close() {
        closed = true
        invalidate()
        port?.close()
        hub = null
        mutableState.value = LocalOutputState()
    }
}
