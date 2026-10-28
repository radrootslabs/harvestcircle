package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

sealed interface HubViewState {
    data object Unavailable : HubViewState

    data object SignedOut : HubViewState

    data object Loading : HubViewState

    data object NeedsSetup : HubViewState

    data class Ready(
        val context: HubContext,
    ) : HubViewState

    data class Failed(
        val message: String,
        val submittedOperation: OperationId?,
    ) : HubViewState
}

/** UI-scope coordinator; call on the owning UI dispatcher. Authorization stays native. */
class HubPresenter(
    private val runtime: HarvestCircleHubRuntime?,
    private val scope: CoroutineScope,
    private val operationIds: OperationIdSource,
) {
    private val mutableState = MutableStateFlow<HubViewState>(HubViewState.Unavailable)
    val state = mutableState.asStateFlow()
    private var identity: IdentityId? = null
    private var generation = 0L
    private var job: Job? = null
    private var submittedOperation: OperationId? = null

    fun bindIdentity(next: IdentityId?) {
        generation += 1
        job?.cancel()
        identity = next
        submittedOperation = null
        mutableState.value =
            when {
                runtime?.demoProfile != true -> HubViewState.Unavailable
                next == null -> HubViewState.SignedOut
                else -> HubViewState.Loading
            }
        if (mutableState.value == HubViewState.Loading) refresh()
    }

    fun refresh() = launch { requireNotNull(runtime).hubContext() }

    fun initialize(revision: SnapshotRevision) {
        if (mutableState.value != HubViewState.NeedsSetup || submittedOperation != null) return
        val operation = operationIds.next()
        submittedOperation = operation
        launch { requireNotNull(runtime).initializeHub(RequestContext(operation, revision, 10_000UL)) }
    }

    private fun launch(operation: suspend () -> HubContext?) {
        if (identity == null || runtime?.demoProfile != true || job?.isActive == true) return
        val requestGeneration = generation
        mutableState.value = HubViewState.Loading
        job =
            scope.launch {
                try {
                    val result = operation()
                    if (requestGeneration == generation) {
                        mutableState.value =
                            when {
                                result != null -> HubViewState.Ready(result)
                                submittedOperation == null -> HubViewState.NeedsSetup
                                else ->
                                    HubViewState.Failed(
                                        "Setup outcome is unresolved. Keep the original operation ID. Review before starting another run.",
                                        submittedOperation,
                                    )
                            }
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (requestGeneration == generation) {
                        mutableState.value =
                            HubViewState.Failed(
                                (error as? ApplicationFailure)?.problem?.safeMessage
                                    ?: "The local hub could not be read. Check native status.",
                                submittedOperation,
                            )
                    }
                }
            }
    }

    fun close() {
        generation += 1
        job?.cancel()
        identity = null
        submittedOperation = null
        mutableState.value = HubViewState.SignedOut
    }
}
