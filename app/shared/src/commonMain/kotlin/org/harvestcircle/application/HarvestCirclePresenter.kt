package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
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
import kotlinx.coroutines.withContext

class HarvestCirclePresenter(
    private val runtime: HarvestCircleRuntime,
    private val scope: CoroutineScope,
    private val clock: ApplicationClock,
    private val operationIds: OperationIdSource,
) : IdentityPresentationPort {
    private val mutableState = MutableStateFlow(HarvestCirclePresenterState(runtime.currentSnapshot()))
    private val mutableEffects = MutableSharedFlow<HarvestCircleEffect>(extraBufferCapacity = EFFECT_BUFFER_CAPACITY)
    private var subscriptionJob: Job? = null
    private var commandJob: Job? = null
    private var pendingRecovery: GeneratedIdentityRecovery? = null
    private var pendingRemoval: PendingRemovalLease? = null
    private var pendingRetry: PendingRetry? = null
    private val removalMutex = Mutex()
    private val closeMutex = Mutex()
    private var closeReceipt: ShutdownReceipt? = null
    private var closePhase = PresenterClosePhase.Open
    private val closed: Boolean
        get() = closePhase != PresenterClosePhase.Open

    override val state: StateFlow<HarvestCirclePresenterState> = mutableState.asStateFlow()
    val effects: SharedFlow<HarvestCircleEffect> = mutableEffects.asSharedFlow()
    val buildInfo: BuildInfo = runtime.buildInfo

    init {
        subscriptionJob =
            scope.launch {
                try {
                    runtime.changes().collect(::acceptChange)
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

    override fun dispatch(intent: HarvestCircleIntent) {
        when (intent) {
            is HarvestCircleIntent.EditImportDraft -> editImportDraft(intent)
            HarvestCircleIntent.ChooseCreateIdentity -> updateState { copy(identityEntryMode = IdentityEntryMode.CREATE, problem = null) }
            HarvestCircleIntent.ChooseImportIdentity -> updateState { copy(identityEntryMode = IdentityEntryMode.IMPORT, problem = null) }
            HarvestCircleIntent.CancelIdentityEntry ->
                clearImportDraft {
                    copy(
                        identityEntryMode = IdentityEntryMode.CHOICE,
                        problem = null,
                    )
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
            is HarvestCircleIntent.CancelIdentityRemoval -> cancelIdentityRemoval(intent)
            is HarvestCircleIntent.ConfirmIdentityRemoval -> confirmIdentityRemoval(intent)
            HarvestCircleIntent.DismissProblem -> updateState { copy(problem = null) }
        }
    }

    suspend fun close(): ShutdownReceipt? =
        closeMutex.withLock {
            closeReceipt?.let { return@withLock it }
            if (closePhase == PresenterClosePhase.Open) {
                closePhase = PresenterClosePhase.Closing
                updateState { copy(route = HarvestCircleRoute.SHUTTING_DOWN, busy = true, problem = null) }
            }
            if (closePhase == PresenterClosePhase.Closing) {
                commandJob?.cancelAndJoin()
                subscriptionJob?.cancelAndJoin()
                releaseRecovery()
                clearImportDraft()
                transferRemovalToShutdown()
                closePhase = PresenterClosePhase.TransferredToShutdown
            }
            return try {
                runtime.shutdown().also { receipt ->
                    if (receipt.closed) {
                        closeReceipt = receipt
                        closePhase = PresenterClosePhase.Closed
                    }
                    updateState {
                        copy(
                            route = if (receipt.closed) HarvestCircleRoute.CLOSED else HarvestCircleRoute.FATAL,
                            busy = false,
                            problem = if (receipt.closed) null else "The application could not shut down safely.",
                        )
                    }
                }
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                acceptFailure(error, null)
                updateState { copy(route = HarvestCircleRoute.FATAL, busy = false) }
                null
            }
        }

    private fun editImportDraft(intent: HarvestCircleIntent.EditImportDraft) {
        val incoming = intent.takeDraft() ?: return
        if (closed) {
            incoming.clear()
            rejectUnavailable("The application runtime is closed.")
            return
        }
        while (true) {
            val current = mutableState.value
            val updated = current.copy(importDraft = incoming, lastProblem = null, problem = null)
            if (!mutableState.compareAndSet(current, updated)) continue
            if (current.importDraft !== incoming) current.importDraft.clear()
            return
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
                updateState { copy(identityEntryMode = IdentityEntryMode.CHOICE) }
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
            onAccepted = { detachImportDraft(draft) },
        ) {
            var input: SecretKeyInput? = null
            try {
                input = SecretKeyInput.from(draft.take())
                val command = ApplicationCommand.ImportLocalIdentity(input, requestContext(operationId))
                acceptResult(runtime.execute(command), operationId)
                updateState { copy(identityEntryMode = IdentityEntryMode.CHOICE) }
            } finally {
                input?.clear()
                draft.clear()
            }
        }
    }

    private fun executeIntent(
        intent: HarvestCircleIntent,
        operationId: OperationId = operationIds.next(),
    ) {
        val activationTarget = (intent as? HarvestCircleIntent.ActivateIdentity)?.identityId
        launchOperation(
            operationId = operationId,
            onAccepted = {
                activationTarget?.let { target ->
                    updateState { copy(activatingIdentityId = target) }
                }
            },
        ) {
            try {
                val command = intent.toApplicationCommand()
                acceptResult(runtime.execute(command), operationId)
                pendingRetry = null
                if (activationTarget != null || intent == HarvestCircleIntent.SignOut) {
                    updateState { copy(identityChooserVisible = false) }
                }
            } catch (error: Exception) {
                val problem = error.toProblem(operationId)
                pendingRetry = PendingRetry(intent, operationId).takeIf { problem.retryable }
                throw ApplicationFailure(problem)
            } finally {
                activationTarget?.let { target ->
                    updateState {
                        if (activatingIdentityId == target) copy(activatingIdentityId = null) else this
                    }
                }
            }
        }
    }

    private fun retryLastCommand() {
        val retry = pendingRetry ?: return rejectUnavailable("This action cannot be retried safely.")
        executeIntent(retry.intent, retry.operationId)
    }

    private fun requestIdentityRemoval(intent: HarvestCircleIntent.RequestIdentityRemoval) {
        launchOperation {
            releaseCurrentRemovalForReplacement()
            updateState {
                copy(
                    removalConfirmation = null,
                    removalStatus = RemovalStatus.NONE,
                )
            }
            val request = runtime.requestIdentityRemoval(intent.identityId)
            val lease = PendingRemovalLease(request)
            val expired = request.isExpired()
            removalMutex.withLock {
                pendingRemoval = lease
                if (expired) {
                    lease.phase = RemovalLeasePhase.Releasing
                    lease.releaseReason = RemovalReleaseReason.Expired
                } else {
                    lease.expiryJob = scope.launch { expireRemovalLease(lease) }
                }
            }
            if (expired) {
                releaseClaimedRemoval(ClaimedRemovalRelease(lease, RemovalReleaseReason.Expired), RemovalStatus.FAILED)
                throw ApplicationFailure(removalExpiredProblem())
            }
            updateState {
                copy(
                    removalConfirmation = request.toConfirmation(),
                    removalStatus = RemovalStatus.AWAITING_CONFIRMATION,
                )
            }
        }
    }

    private fun cancelIdentityRemoval(intent: HarvestCircleIntent.CancelIdentityRemoval) {
        launchOperation {
            val claim =
                claimMatchingRemoval(intent.identityId, intent.requestId, RemovalTerminalAction.Cancel)
                    ?: return@launchOperation
            releaseClaimedRemoval(
                ClaimedRemovalRelease(claim.lease, claim.releaseReason ?: RemovalReleaseReason.UserCancelled),
                if (claim.expired) RemovalStatus.FAILED else RemovalStatus.NONE,
            )
            if (claim.expired) {
                throw ApplicationFailure(removalExpiredProblem())
            }
        }
    }

    private fun confirmIdentityRemoval(intent: HarvestCircleIntent.ConfirmIdentityRemoval) {
        val operationId = operationIds.next()
        launchOperation(operationId = operationId) {
            val claim =
                claimMatchingRemoval(intent.identityId, intent.requestId, RemovalTerminalAction.Confirm)
                    ?: return@launchOperation
            val lease = claim.lease
            val request = lease.request
            if (claim.expired) {
                releaseClaimedRemoval(ClaimedRemovalRelease(lease, RemovalReleaseReason.Expired), RemovalStatus.FAILED)
                throw ApplicationFailure(removalExpiredProblem(operationId))
            }
            updateState {
                copy(
                    removalConfirmation = null,
                    removalStatus = RemovalStatus.CONFIRMING,
                )
            }
            try {
                val result =
                    runtime.execute(
                        ApplicationCommand.ConfirmIdentityRemoval(
                            request.requestId,
                            requestContext(operationId),
                        ),
                    )
                acceptResult(result, operationId)
                removalMutex.withLock {
                    if (pendingRemoval === lease && lease.phase == RemovalLeasePhase.Confirming) {
                        lease.phase = RemovalLeasePhase.Confirmed
                        pendingRemoval = null
                    }
                }
                updateState {
                    copy(
                        removalConfirmation = null,
                        removalStatus = RemovalStatus.COMPLETED,
                        lastRemovedIdentityId = request.identityId,
                    )
                }
                mutableEffects.tryEmit(HarvestCircleEffect.IdentityRemoved(request.identityId))
            } catch (error: CancellationException) {
                withContext(NonCancellable) {
                    releaseAfterFailedConfirmation(lease)
                }
                throw error
            } catch (error: Exception) {
                releaseAfterFailedConfirmation(lease)
                throw error
            }
        }
    }

    private suspend fun claimMatchingRemoval(
        identityId: IdentityId,
        requestId: RemovalRequestId,
        action: RemovalTerminalAction,
    ): ClaimedRemoval? {
        var rejection: String? = null
        var releaseFailed = false
        val claim =
            removalMutex.withLock {
                val lease = pendingRemoval
                val confirmation = state.value.removalConfirmation
                if (lease?.phase == RemovalLeasePhase.ReleaseFailed) {
                    releaseFailed = true
                    return@withLock null
                }
                if (lease == null ||
                    lease.phase != RemovalLeasePhase.Open ||
                    confirmation == null ||
                    state.value.removalStatus != RemovalStatus.AWAITING_CONFIRMATION
                ) {
                    rejection = "Identity removal confirmation is not available."
                    return@withLock null
                }
                val request = lease.request
                if (request.identityId != identityId ||
                    request.requestId != requestId ||
                    confirmation.identityId != identityId ||
                    confirmation.requestId != requestId
                ) {
                    rejection = "Identity removal confirmation does not match the current request."
                    return@withLock null
                }
                val expired = request.isExpired()
                val releaseReason =
                    when {
                        expired -> RemovalReleaseReason.Expired
                        action == RemovalTerminalAction.Cancel -> RemovalReleaseReason.UserCancelled
                        else -> null
                    }
                lease.phase = if (releaseReason == null) RemovalLeasePhase.Confirming else RemovalLeasePhase.Releasing
                lease.releaseReason = releaseReason
                val expiryJob = lease.expiryJob
                lease.expiryJob = null
                ClaimedRemoval(lease, expiryJob, expired, releaseReason)
            }
        if (releaseFailed) {
            acceptFailure(ApplicationFailure(removalReleaseFailedProblem()), null)
            return null
        }
        if (claim == null) {
            rejectUnavailable(checkNotNull(rejection))
            return null
        }
        claim.expiryJob?.cancelAndJoin()
        return claim
    }

    private suspend fun releaseCurrentRemovalForReplacement() {
        var releaseFailed = false
        val claim =
            removalMutex.withLock {
                val lease = pendingRemoval ?: return@withLock null
                if (lease.phase == RemovalLeasePhase.ReleaseFailed) {
                    releaseFailed = true
                    return@withLock null
                }
                if (lease.phase != RemovalLeasePhase.Open) {
                    throw ApplicationFailure(removalReleaseFailedProblem())
                }
                lease.phase = RemovalLeasePhase.Releasing
                lease.releaseReason = RemovalReleaseReason.Replaced
                val expiryJob = lease.expiryJob
                lease.expiryJob = null
                ClaimedRemoval(lease, expiryJob, expired = false, RemovalReleaseReason.Replaced)
            }
        if (releaseFailed) throw ApplicationFailure(removalReleaseFailedProblem())
        claim ?: return
        claim.expiryJob?.cancelAndJoin()
        releaseClaimedRemoval(
            ClaimedRemovalRelease(claim.lease, RemovalReleaseReason.Replaced),
            RemovalStatus.NONE,
        )
    }

    private suspend fun releaseAfterFailedConfirmation(lease: PendingRemovalLease) {
        val claimed =
            removalMutex.withLock {
                if (pendingRemoval !== lease || lease.phase != RemovalLeasePhase.Confirming) return@withLock false
                lease.phase = RemovalLeasePhase.Releasing
                lease.releaseReason = RemovalReleaseReason.ConfirmFailed
                true
            }
        if (claimed) {
            releaseClaimedRemoval(
                ClaimedRemovalRelease(lease, RemovalReleaseReason.ConfirmFailed),
                RemovalStatus.FAILED,
            )
        }
    }

    private suspend fun releaseClaimedRemoval(
        claim: ClaimedRemovalRelease,
        resultingStatus: RemovalStatus,
    ) {
        val lease = claim.lease
        val released =
            try {
                runtime.cancelIdentityRemoval(lease.request.requestId)
            } catch (error: CancellationException) {
                quarantineRemovalLease(claim)
                if (!closed) acceptFailure(ApplicationFailure(removalReleaseFailedProblem()), null)
                throw error
            } catch (_: Exception) {
                quarantineRemovalLease(claim)
                throw ApplicationFailure(removalReleaseFailedProblem())
            }
        if (!released) {
            quarantineRemovalLease(claim)
            throw ApplicationFailure(removalReleaseFailedProblem())
        }
        val completed =
            removalMutex.withLock {
                if (pendingRemoval !== lease ||
                    lease.phase != RemovalLeasePhase.Releasing ||
                    lease.releaseReason != claim.reason
                ) {
                    return@withLock false
                }
                lease.phase = RemovalLeasePhase.Released
                pendingRemoval = null
                true
            }
        if (!completed) throw ApplicationFailure(removalReleaseFailedProblem())
        updateState {
            copy(
                removalConfirmation = null,
                removalStatus = resultingStatus,
            )
        }
    }

    private suspend fun quarantineRemovalLease(claim: ClaimedRemovalRelease) {
        val publish =
            removalMutex.withLock {
                val lease = claim.lease
                if (pendingRemoval !== lease ||
                    lease.phase != RemovalLeasePhase.Releasing ||
                    lease.releaseReason != claim.reason
                ) {
                    return@withLock false
                }
                lease.phase = RemovalLeasePhase.ReleaseFailed
                closePhase == PresenterClosePhase.Open
            }
        if (publish) {
            updateState {
                copy(
                    removalConfirmation = null,
                    removalStatus = RemovalStatus.FAILED,
                )
            }
        }
    }

    private suspend fun expireRemovalLease(lease: PendingRemovalLease) {
        while (true) {
            delay(removalExpiryDelayMillis(lease.request.expiresAt, clock.now()))
            var stillOpenBeforeDeadline = false
            val claimed =
                removalMutex.withLock {
                    if (pendingRemoval !== lease ||
                        lease.phase != RemovalLeasePhase.Open ||
                        closePhase != PresenterClosePhase.Open
                    ) {
                        return@withLock false
                    }
                    if (!lease.request.isExpired()) {
                        stillOpenBeforeDeadline = true
                        return@withLock false
                    }
                    lease.phase = RemovalLeasePhase.Releasing
                    lease.releaseReason = RemovalReleaseReason.Expired
                    true
                }
            if (!claimed) {
                if (stillOpenBeforeDeadline) continue
                return
            }
            updateState { copy(removalConfirmation = null) }
            try {
                releaseClaimedRemoval(
                    ClaimedRemovalRelease(lease, RemovalReleaseReason.Expired),
                    RemovalStatus.FAILED,
                )
                acceptFailure(ApplicationFailure(removalExpiredProblem()), null)
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                acceptFailure(error, null)
            }
            return
        }
    }

    private suspend fun transferRemovalToShutdown() {
        val expiryJob =
            removalMutex.withLock {
                pendingRemoval?.let { lease ->
                    lease.expiryJob.also { lease.expiryJob = null }
                }
            }
        expiryJob?.cancelAndJoin()
        removalMutex.withLock {
            pendingRemoval?.let { lease ->
                lease.phase = RemovalLeasePhase.TransferredToShutdown
                lease.releaseReason = null
            }
            pendingRemoval = null
        }
        updateState { copy(removalConfirmation = null) }
    }

    private fun IdentityRemovalRequest.isExpired(): Boolean = expiresAt.value <= clock.now().value

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

    private fun acceptChange(change: ApplicationChange) {
        val acceptedRevision = state.value.snapshot.revision
        if (change.snapshot.revision.value <= acceptedRevision.value) return
        if (change.previousRevision == acceptedRevision) {
            acceptSnapshot(change.snapshot)
            return
        }
        val refreshed = runtime.currentSnapshot()
        if (refreshed.revision.value < change.snapshot.revision.value) {
            throw ApplicationFailure(
                ApplicationProblem(
                    code = ApplicationErrorCode.ObserverRegistrationFailed,
                    category = ApplicationErrorCategory.Lifecycle,
                    retryable = false,
                    recoveryAction = RecoveryAction.RestartApplication,
                    operationId = null,
                    safeMessage = "Application updates could not be synchronized.",
                ),
            )
        }
        acceptSnapshot(refreshed)
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
        val recovery = pendingRecovery
        pendingRecovery = null
        recovery?.backup?.clear()
        updateState { copy(generatedKeyBackup = null) }
    }

    private fun detachImportDraft(draft: ImportSecretDraft) {
        while (true) {
            val current = mutableState.value
            if (current.importDraft !== draft) return
            if (mutableState.compareAndSet(current, current.copy(importDraft = ImportSecretDraft.empty()))) return
        }
    }

    private fun clearImportDraft(transform: HarvestCirclePresenterState.() -> HarvestCirclePresenterState = { this }) {
        while (true) {
            val current = mutableState.value
            val replacement = ImportSecretDraft.empty()
            val updated = current.copy(importDraft = replacement).transform()
            if (!mutableState.compareAndSet(current, updated)) {
                replacement.clear()
                continue
            }
            current.importDraft.clear()
            return
        }
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

private class PendingRemovalLease(
    val request: IdentityRemovalRequest,
    var phase: RemovalLeasePhase = RemovalLeasePhase.Open,
    var releaseReason: RemovalReleaseReason? = null,
    var expiryJob: Job? = null,
)

private enum class RemovalLeasePhase {
    Open,
    Confirming,
    Releasing,
    Confirmed,
    Released,
    ReleaseFailed,
    TransferredToShutdown,
}

private enum class RemovalReleaseReason {
    Expired,
    UserCancelled,
    Replaced,
    ConfirmFailed,
}

private enum class RemovalTerminalAction {
    Confirm,
    Cancel,
}

private enum class PresenterClosePhase {
    Open,
    Closing,
    TransferredToShutdown,
    Closed,
}

private data class ClaimedRemoval(
    val lease: PendingRemovalLease,
    val expiryJob: Job?,
    val expired: Boolean,
    val releaseReason: RemovalReleaseReason?,
)

private data class ClaimedRemovalRelease(
    val lease: PendingRemovalLease,
    val reason: RemovalReleaseReason,
)

private fun IdentityRemovalRequest.toConfirmation(): IdentityRemovalConfirmation =
    IdentityRemovalConfirmation(
        identityId = identityId,
        requestId = requestId,
        deletesLocalCredential = deletesLocalCredential,
        signsOut = signsOut,
        expiresAt = expiresAt,
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

private fun removalReleaseFailedProblem(): ApplicationProblem =
    ApplicationProblem(
        code = ApplicationErrorCode.InvalidApplicationState,
        category = ApplicationErrorCategory.Lifecycle,
        retryable = false,
        recoveryAction = RecoveryAction.RestartApplication,
        operationId = null,
        safeMessage = "The identity removal request could not be released safely.",
    )

private fun removalExpiredProblem(operationId: OperationId? = null): ApplicationProblem =
    ApplicationProblem(
        code = ApplicationErrorCode.InvalidApplicationState,
        category = ApplicationErrorCategory.Lifecycle,
        retryable = false,
        recoveryAction = RecoveryAction.None,
        operationId = operationId,
        safeMessage = "Identity removal confirmation has expired.",
    )

private val READY_ROUTES =
    setOf(
        HarvestCircleRoute.IDENTITIES,
        HarvestCircleRoute.ACTIVE_IDENTITY,
        HarvestCircleRoute.DEGRADED,
    )
private const val EFFECT_BUFFER_CAPACITY = 8
private const val COMMAND_DEADLINE_MILLIS = 5_000UL

internal fun removalExpiryDelayMillis(
    expiresAt: UnixSeconds,
    now: UnixSeconds,
): Long {
    if (expiresAt.value <= now.value) return 0L
    val remainingSeconds =
        if (now.value < 0L && expiresAt.value > Long.MAX_VALUE + now.value) {
            Long.MAX_VALUE
        } else {
            expiresAt.value - now.value
        }
    return if (remainingSeconds > Long.MAX_VALUE / MILLIS_PER_SECOND) {
        Long.MAX_VALUE
    } else {
        remainingSeconds * MILLIS_PER_SECOND
    }
}

private const val MILLIS_PER_SECOND = 1_000L
