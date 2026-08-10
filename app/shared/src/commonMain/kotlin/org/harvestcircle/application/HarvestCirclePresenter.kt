package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

class HarvestCirclePresenter(
    private val runtime: HarvestCircleRuntime,
    private val scope: CoroutineScope,
    private val clock: ApplicationClock,
    private val operationIds: OperationIdSource,
) {
    private val mutableState = MutableStateFlow(HarvestCirclePresenterState(runtime.currentSnapshot()))
    private val mutableEffects = MutableSharedFlow<HarvestCircleEffect>(extraBufferCapacity = EFFECT_BUFFER_CAPACITY)
    private var subscriptionJob: Job? = null
    private var commandJob: Job? = null
    private var pendingRecovery: GeneratedIdentityRecovery? = null
    private var pendingRemoval: IdentityRemovalRequest? = null
    private var pendingRetry: PendingRetry? = null
    private val closeMutex = Mutex()
    private var closeReceipt: ShutdownReceipt? = null
    private var closed = false

    val state: StateFlow<HarvestCirclePresenterState> = mutableState.asStateFlow()
    val effects: SharedFlow<HarvestCircleEffect> = mutableEffects.asSharedFlow()

    init {
        subscriptionJob =
            scope.launch {
                try {
                    runtime.changes().collect { change -> acceptSnapshot(change.snapshot) }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    if (!closed) acceptFailure(error, null)
                }
            }
        launchOperation(requireReady = false) {
            acceptSnapshot(runtime.bootstrap())
        }
    }

    fun dispatch(intent: HarvestCircleIntent) {
        when (intent) {
            is HarvestCircleIntent.EditImportDraft -> editImportDraft(intent.value)
            HarvestCircleIntent.ChooseCreateIdentity -> updateState { copy(identityEntryMode = IdentityEntryMode.CREATE, problem = null) }
            HarvestCircleIntent.ChooseImportIdentity -> updateState { copy(identityEntryMode = IdentityEntryMode.IMPORT, problem = null) }
            HarvestCircleIntent.CancelIdentityEntry ->
                updateState {
                    copy(identityEntryMode = IdentityEntryMode.CHOICE, importDraft = "", problem = null)
                }
            HarvestCircleIntent.GenerateIdentity -> prepareIdentity()
            HarvestCircleIntent.AcknowledgeGeneratedRecovery -> acknowledgeRecovery()
            HarvestCircleIntent.CancelGeneratedRecovery -> cancelRecovery()
            HarvestCircleIntent.ImportIdentity -> importIdentity()
            is HarvestCircleIntent.SelectIdentity -> executeIntent(intent)
            is HarvestCircleIntent.ActivateIdentity -> executeIntent(intent)
            HarvestCircleIntent.SignOut -> executeIntent(intent)
            HarvestCircleIntent.ShowIdentityChooser -> updateState { copy(identityChooserVisible = true, problem = null) }
            HarvestCircleIntent.HideIdentityChooser -> updateState { copy(identityChooserVisible = false) }
            HarvestCircleIntent.RefreshActiveProfile -> executeIntent(intent)
            HarvestCircleIntent.RetryLastCommand -> retryLastCommand()
            is HarvestCircleIntent.RequestIdentityRemoval -> requestIdentityRemoval(intent)
            HarvestCircleIntent.CancelIdentityRemoval -> cancelIdentityRemoval()
            HarvestCircleIntent.ConfirmIdentityRemoval -> confirmIdentityRemoval()
            HarvestCircleIntent.DismissProblem -> updateState { copy(problem = null) }
        }
    }

    suspend fun close(): ShutdownReceipt? =
        closeMutex.withLock {
            closeReceipt?.let { return@withLock it }
            if (closed) return@withLock null
            closed = true
            updateState { copy(route = HarvestCircleRoute.SHUTTING_DOWN, busy = true, problem = null) }
            commandJob?.cancelAndJoin()
            subscriptionJob?.cancelAndJoin()
            releaseRecovery()
            pendingRemoval = null
            return try {
                runtime.shutdown().also { receipt ->
                    closeReceipt = receipt
                    updateState {
                        copy(
                            route = if (receipt.closed) HarvestCircleRoute.CLOSED else HarvestCircleRoute.FATAL,
                            busy = false,
                            problem = if (receipt.closed) null else "The application could not shut down safely.",
                        )
                    }
                }
            } catch (error: Exception) {
                acceptFailure(error, null)
                updateState { copy(route = HarvestCircleRoute.FATAL, busy = false) }
                null
            }
        }

    private fun editImportDraft(value: String) {
        updateState {
            copy(
                importDraft = value.take(MAX_IMPORT_SECRET_CHARS),
                lastProblem = null,
                problem = null,
            )
        }
    }

    private fun prepareIdentity() {
        launchOperation {
            pendingRecovery?.let { previous ->
                runtime.execute(ApplicationCommand.CancelGeneratedIdentity(previous.requestId))
                previous.backup.clear()
            }
            val recovery = runtime.prepareLocalIdentity()
            pendingRecovery = recovery
            updateState { copy(generatedKeyBackup = recovery.backup) }
        }
    }

    private fun acknowledgeRecovery() {
        val recovery = pendingRecovery ?: return rejectUnavailable("Generated-key recovery is not available.")
        val operationId = operationIds.next()
        launchOperation(operationId = operationId) {
            try {
                val result =
                    runtime.execute(
                        ApplicationCommand.AcknowledgeGeneratedIdentity(
                            recovery.requestId,
                            requestContext(operationId),
                        ),
                    )
                acceptResult(result, operationId)
            } finally {
                releaseRecovery()
            }
        }
    }

    private fun cancelRecovery() {
        val recovery = pendingRecovery ?: return rejectUnavailable("Generated-key recovery is not available.")
        launchOperation {
            try {
                runtime.execute(ApplicationCommand.CancelGeneratedIdentity(recovery.requestId))
            } finally {
                releaseRecovery()
            }
        }
    }

    private fun importIdentity() {
        val draft = state.value.importDraft
        val operationId = operationIds.next()
        launchOperation(
            operationId = operationId,
            onAccepted = { updateState { copy(importDraft = "") } },
        ) {
            val input = SecretKeyInput.from(draft)
            val command = ApplicationCommand.ImportLocalIdentity(input, requestContext(operationId))
            try {
                acceptResult(runtime.execute(command), operationId)
            } finally {
                input.clear()
            }
        }
    }

    private fun executeIntent(
        intent: HarvestCircleIntent,
        operationId: OperationId = operationIds.next(),
    ) {
        launchOperation(operationId = operationId) {
            val command = intent.toApplicationCommand()
            try {
                acceptResult(runtime.execute(command), operationId)
                pendingRetry = null
                if (intent is HarvestCircleIntent.ActivateIdentity || intent == HarvestCircleIntent.SignOut) {
                    updateState { copy(identityChooserVisible = false) }
                }
            } catch (error: Exception) {
                val problem = error.toProblem(operationId)
                pendingRetry = PendingRetry(intent, operationId).takeIf { problem.retryable }
                throw ApplicationFailure(problem)
            }
        }
    }

    private fun retryLastCommand() {
        val retry = pendingRetry ?: return rejectUnavailable("This action cannot be retried safely.")
        executeIntent(retry.intent, retry.operationId)
    }

    private fun requestIdentityRemoval(intent: HarvestCircleIntent.RequestIdentityRemoval) {
        launchOperation {
            pendingRemoval?.let { runtime.cancelIdentityRemoval(it.requestId) }
            val request = runtime.requestIdentityRemoval(intent.identityId)
            check(request.expiresAt.value > clock.now().value) { "Identity removal request is already expired" }
            pendingRemoval = request
            updateState {
                copy(
                    pendingRemovalIdentityId = request.identityId,
                    removalImpact =
                        RemovalImpactState(
                            request.identityId,
                            request.deletesLocalCredential,
                            request.signsOut,
                            request.expiresAt,
                        ),
                    removalStatus = RemovalStatus.AWAITING_CONFIRMATION,
                )
            }
        }
    }

    private fun cancelIdentityRemoval() {
        val request = pendingRemoval ?: return rejectUnavailable("Identity removal confirmation is not available.")
        launchOperation {
            runtime.cancelIdentityRemoval(request.requestId)
            pendingRemoval = null
            updateState {
                copy(
                    pendingRemovalIdentityId = null,
                    removalImpact = null,
                    removalStatus = RemovalStatus.NONE,
                )
            }
        }
    }

    private fun confirmIdentityRemoval() {
        val request = pendingRemoval ?: return rejectUnavailable("Identity removal confirmation is not available.")
        val operationId = operationIds.next()
        launchOperation(
            operationId = operationId,
            onAccepted = {
                pendingRemoval = null
                updateState { copy(removalStatus = RemovalStatus.CONFIRMING) }
            },
        ) {
            try {
                val result =
                    runtime.execute(
                        ApplicationCommand.ConfirmIdentityRemoval(
                            request.requestId,
                            requestContext(operationId),
                        ),
                    )
                acceptResult(result, operationId)
                updateState {
                    copy(
                        pendingRemovalIdentityId = null,
                        removalImpact = null,
                        removalStatus = RemovalStatus.COMPLETED,
                        lastRemovedIdentityId = request.identityId,
                    )
                }
                mutableEffects.tryEmit(HarvestCircleEffect.IdentityRemoved(request.identityId))
            } finally {
                if (state.value.removalStatus != RemovalStatus.COMPLETED) {
                    runtime.cancelIdentityRemoval(request.requestId)
                    updateState {
                        copy(
                            pendingRemovalIdentityId = null,
                            removalImpact = null,
                            removalStatus = RemovalStatus.FAILED,
                        )
                    }
                }
            }
        }
    }

    private fun launchOperation(
        requireReady: Boolean = true,
        operationId: OperationId? = null,
        onAccepted: () -> Unit = {},
        operation: suspend () -> Unit,
    ) {
        if (rejectIfUnavailable(requireReady)) return
        updateState {
            copy(
                busy = true,
                commandStatus = CommandStatus.RUNNING,
                lastCommandOperationId = operationId ?: lastCommandOperationId,
                problem = null,
            )
        }
        onAccepted()
        commandJob =
            scope.launch {
                try {
                    operation()
                    if (state.value.commandStatus == CommandStatus.RUNNING) {
                        updateState { copy(commandStatus = CommandStatus.ACCEPTED) }
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    acceptFailure(error, operationId)
                } finally {
                    updateState { copy(busy = false) }
                }
            }
    }

    private fun rejectIfUnavailable(requireReady: Boolean): Boolean {
        val current = state.value
        if (closed) {
            updateState {
                copy(commandStatus = CommandStatus.REJECTED_CLOSED, problem = "The application runtime is closed.")
            }
            return true
        }
        if (commandJob?.isActive == true) {
            updateState {
                copy(commandStatus = CommandStatus.REJECTED_BUSY, problem = "The application is busy. Try again.")
            }
            return true
        }
        if (requireReady && current.route !in READY_ROUTES) {
            updateState {
                copy(commandStatus = CommandStatus.FAILED_TERMINAL, problem = "The application runtime is not ready for this action.")
            }
            return true
        }
        return false
    }

    private fun rejectUnavailable(message: String) {
        updateState {
            copy(
                commandStatus = if (closed) CommandStatus.REJECTED_CLOSED else CommandStatus.FAILED_TERMINAL,
                problem = message,
            )
        }
    }

    private fun acceptResult(
        result: ApplicationCommandResult,
        operationId: OperationId,
    ) {
        acceptSnapshot(result.snapshot)
        updateState {
            copy(
                commandStatus = CommandStatus.ACCEPTED,
                lastCommandOperationId = operationId,
                lastProblem = null,
                problem = null,
            )
        }
    }

    private fun acceptSnapshot(snapshot: ApplicationSnapshot) {
        if (snapshot.revision.value >= state.value.snapshot.revision.value) {
            updateState { copy(snapshot = snapshot, route = snapshot.toHarvestCircleRoute()) }
        }
    }

    private fun acceptFailure(
        error: Throwable,
        operationId: OperationId?,
    ) {
        val problem = error.toProblem(operationId)
        updateState {
            copy(
                commandStatus = if (problem.retryable) CommandStatus.FAILED_RETRYABLE else CommandStatus.FAILED_TERMINAL,
                lastCommandOperationId = problem.operationId ?: operationId ?: lastCommandOperationId,
                lastProblem = problem,
                problem = problem.safeMessage,
            )
        }
        mutableEffects.tryEmit(HarvestCircleEffect.Problem(problem))
    }

    private fun releaseRecovery() {
        pendingRecovery?.backup?.clear()
        pendingRecovery = null
        updateState { copy(generatedKeyBackup = null) }
    }

    private fun requestContext(operationId: OperationId): RequestContext =
        RequestContext(operationId, state.value.snapshot.revision, COMMAND_DEADLINE_MILLIS)

    private fun updateState(transform: HarvestCirclePresenterState.() -> HarvestCirclePresenterState) {
        mutableState.update(transform)
    }
}

private data class PendingRetry(
    val intent: HarvestCircleIntent,
    val operationId: OperationId,
)

private fun HarvestCircleIntent.toApplicationCommand(): ApplicationCommand =
    when (this) {
        is HarvestCircleIntent.SelectIdentity -> ApplicationCommand.SelectIdentity(identityId)
        is HarvestCircleIntent.ActivateIdentity -> ApplicationCommand.ActivateIdentity(identityId)
        HarvestCircleIntent.SignOut -> ApplicationCommand.SignOut
        HarvestCircleIntent.RefreshActiveProfile -> ApplicationCommand.RefreshActiveProfile
        else -> error("Intent is not a direct application command")
    }

private fun Throwable.toProblem(operationId: OperationId?): ApplicationProblem =
    (this as? ApplicationFailure)?.problem
        ?: ApplicationProblem(
            code = ApplicationErrorCode.Internal,
            category = ApplicationErrorCategory.Internal,
            retryable = false,
            recoveryAction = RecoveryAction.None,
            operationId = operationId,
            safeMessage = "The application command failed.",
        )

private val READY_ROUTES = setOf(HarvestCircleRoute.IDENTITIES, HarvestCircleRoute.ACTIVE_IDENTITY)
private const val EFFECT_BUFFER_CAPACITY = 8
private const val COMMAND_DEADLINE_MILLIS = 5_000UL
