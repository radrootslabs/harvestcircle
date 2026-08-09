package org.radroots.harvestcircle.application

import androidx.compose.runtime.State
import androidx.compose.runtime.mutableStateOf
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.radroots.harvestcircle.ffi.AppLifecycleDto
import org.radroots.harvestcircle.ffi.AppSnapshotDto
import org.radroots.harvestcircle.ffi.StudioException
import org.radroots.harvestcircle.ffi.WireErrorCode
import org.radroots.harvestcircle.ffi.WireRecoveryAction

enum class HarvestCircleRoute {
    OPENING,
    CHECKING_COMPATIBILITY,
    ACQUIRING_OWNERSHIP,
    MIGRATING,
    RECOVERING,
    ACCOUNTS,
    ACTIVE_ACCOUNT,
    DEGRADED,
    BLOCKED,
    SHUTTING_DOWN,
    FATAL,
    CLOSED,
}

enum class CommandStatus {
    IDLE,
    RUNNING,
    ACCEPTED,
    REJECTED_BUSY,
    REJECTED_CLOSED,
    FAILED_RETRYABLE,
    FAILED_TERMINAL,
}

enum class AccountEntryMode {
    CHOICE,
    CREATE,
    IMPORT,
}

enum class RemovalStatus {
    NONE,
    AWAITING_CONFIRMATION,
    CONFIRMING,
    COMPLETED,
    FAILED,
}

data class RemovalImpactState(
    val publicKeyHex: String,
    val deletesLocalCredential: Boolean,
    val signsOut: Boolean,
    val expiresAtSeconds: Long,
)

data class HarvestCircleStoreState(
    val snapshot: AppSnapshotDto,
    val route: HarvestCircleRoute = snapshot.toHarvestCircleRoute(),
    val importDraft: String = "",
    val generatedKeyBackup: GeneratedKeyBackup? = null,
    val pendingRemovalPublicKeyHex: String? = null,
    val removalImpact: RemovalImpactState? = null,
    val removalStatus: RemovalStatus = RemovalStatus.NONE,
    val lastRemovedPublicKeyHex: String? = null,
    val accountChooserVisible: Boolean = false,
    val accountEntryMode: AccountEntryMode = AccountEntryMode.CHOICE,
    val busy: Boolean = false,
    val commandStatus: CommandStatus = CommandStatus.IDLE,
    val lastCommandRequestId: String? = null,
    val lastFailureCode: WireErrorCode? = null,
    val recoveryAction: WireRecoveryAction = WireRecoveryAction.NONE,
    val problem: String? = null,
)

const val MAX_IMPORT_SECRET_CHARS: Int = 128

class HarvestCircleAppStore(
    private val gateway: HarvestCircleCoreGateway,
    private val scope: CoroutineScope,
) : AutoCloseable {
    private val mutableState = mutableStateOf(HarvestCircleStoreState(snapshot = gateway.snapshot()))
    private var closed = false
    private var subscription: AutoCloseable? = null
    private var pendingRemoval: RemovalTicket? = null
    private var pendingGeneratedRecovery: PendingGeneratedRecovery? = null
    private var command: Job? = null
    private var retryableCommand: HarvestCircleCommand? = null

    val state: State<HarvestCircleStoreState>
        get() = mutableState

    init {
        launchCommand {
            val registered =
                gateway.subscribeChanges { change ->
                    scope.launch {
                        if (!closed) acceptSnapshot(change.snapshot)
                    }
                }
            if (closed) {
                registered.close()
                return@launchCommand
            }
            subscription = registered
            acceptSnapshot(gateway.bootstrap())
        }
    }

    fun editImportDraft(value: String) {
        mutableState.value =
            mutableState.value.copy(
                importDraft = value.take(MAX_IMPORT_SECRET_CHARS),
                lastFailureCode = null,
                recoveryAction = WireRecoveryAction.NONE,
                problem = null,
            )
    }

    fun chooseCreateAccount() {
        mutableState.value = mutableState.value.copy(accountEntryMode = AccountEntryMode.CREATE, problem = null)
    }

    fun chooseImportAccount() {
        mutableState.value = mutableState.value.copy(accountEntryMode = AccountEntryMode.IMPORT, problem = null)
    }

    fun cancelAccountEntry() {
        mutableState.value =
            mutableState.value.copy(
                accountEntryMode = AccountEntryMode.CHOICE,
                importDraft = "",
                problem = null,
            )
    }

    fun generateAccount() {
        launchCommand {
            val recovery = gateway.beginGeneratedAccount()
            var installed = false
            try {
                val backup = GeneratedKeyBackup(recovery.account.npub, recovery.takeRecoveryNsec())
                pendingGeneratedRecovery = PendingGeneratedRecovery(recovery, backup)
                mutableState.value = mutableState.value.copy(generatedKeyBackup = backup)
                installed = true
            } finally {
                if (!installed) {
                    withContext(NonCancellable) {
                        runCatching { recovery.cancel() }
                        recovery.close()
                    }
                }
            }
        }
    }

    fun acknowledgeGeneratedKeyBackup() {
        val recovery =
            pendingGeneratedRecovery ?: run {
                rejectUnavailableIntent("Generated-key recovery is not available.")
                return
            }
        runSnapshotCommand {
            try {
                recovery.ticket.acknowledge()
            } finally {
                releaseGeneratedRecovery(recovery)
            }
        }
    }

    fun cancelGeneratedKeyBackup() {
        val recovery =
            pendingGeneratedRecovery ?: run {
                rejectUnavailableIntent("Generated-key recovery is not available.")
                return
            }
        launchCommand {
            try {
                if (!recovery.ticket.cancel()) {
                    throw HarvestCircleGatewayException(
                        HarvestCircleCommandFailure(
                            code = WireErrorCode.INVALID_APPLICATION_STATE,
                            category = org.radroots.harvestcircle.ffi.WireErrorCategory.LIFECYCLE,
                            retryable = false,
                            recoveryAction = WireRecoveryAction.NONE,
                            correlationId = recovery.ticket.requestId,
                            safeMessage = "The generated-key recovery step was already closed.",
                        ),
                    )
                }
            } finally {
                releaseGeneratedRecovery(recovery)
            }
        }
    }

    fun importSecretKey() {
        if (rejectIfUnavailable()) return
        val input = mutableState.value.importDraft.encodeToByteArray()
        mutableState.value = mutableState.value.copy(importDraft = "")
        runTypedCommand(HarvestCircleCommand.ImportAccount(input))
    }

    fun selectAccount(publicKeyHex: String) {
        runTypedCommand(HarvestCircleCommand.SelectAccount(publicKeyHex))
    }

    fun activateAccount(publicKeyHex: String) {
        runTypedCommand(HarvestCircleCommand.ActivateAccount(publicKeyHex), hideChooser = true)
    }

    fun signOut() {
        runTypedCommand(HarvestCircleCommand.SignOut, hideChooser = true)
    }

    fun showAccountChooser() {
        mutableState.value = mutableState.value.copy(accountChooserVisible = true, problem = null)
    }

    fun hideAccountChooser() {
        mutableState.value = mutableState.value.copy(accountChooserVisible = false)
    }

    fun refreshActiveProfile() {
        runTypedCommand(HarvestCircleCommand.RefreshProfile)
    }

    fun retryLastCommand() {
        val retry =
            retryableCommand ?: run {
                rejectUnavailableIntent("This action cannot be retried safely.")
                return
            }
        runTypedCommand(retry)
    }

    fun requestAccountRemoval(publicKeyHex: String) {
        launchCommand {
            runCatching {
                pendingRemoval?.close()
                pendingRemoval = null
                val ticket = gateway.requestAccountRemoval(publicKeyHex)
                if (closed) {
                    ticket.close()
                    return@runCatching
                }
                pendingRemoval = ticket
                mutableState.value =
                    mutableState.value.copy(
                        pendingRemovalPublicKeyHex = publicKeyHex,
                        removalImpact =
                            RemovalImpactState(
                                ticket.publicKeyHex,
                                ticket.deletesLocalCredential,
                                ticket.signsOut,
                                ticket.expiresAtSeconds,
                            ),
                        removalStatus = RemovalStatus.AWAITING_CONFIRMATION,
                    )
            }.getOrThrow()
        }
    }

    fun cancelAccountRemoval() {
        pendingRemoval?.close()
        pendingRemoval = null
        mutableState.value =
            mutableState.value.copy(
                pendingRemovalPublicKeyHex = null,
                removalImpact = null,
                removalStatus = RemovalStatus.NONE,
            )
    }

    fun confirmAccountRemoval() {
        val ticket =
            pendingRemoval ?: run {
                rejectUnavailableIntent("Account removal confirmation is not available.")
                return
            }
        pendingRemoval = null
        mutableState.value = mutableState.value.copy(removalStatus = RemovalStatus.CONFIRMING)
        runSnapshotCommand {
            try {
                gateway.confirmAccountRemoval(ticket).also {
                    mutableState.value =
                        mutableState.value.copy(
                            pendingRemovalPublicKeyHex = null,
                            lastRemovedPublicKeyHex = ticket.publicKeyHex,
                            removalImpact = null,
                            removalStatus = RemovalStatus.COMPLETED,
                        )
                }
            } finally {
                ticket.close()
                if (mutableState.value.removalStatus != RemovalStatus.COMPLETED) {
                    mutableState.value =
                        mutableState.value.copy(
                            pendingRemovalPublicKeyHex = null,
                            removalImpact = null,
                            removalStatus = RemovalStatus.FAILED,
                        )
                }
            }
        }
    }

    fun dismissProblem() {
        mutableState.value = mutableState.value.copy(problem = null)
    }

    private fun runSnapshotCommand(operation: suspend () -> AppSnapshotDto) {
        launchCommand { acceptSnapshot(operation()) }
    }

    private fun runTypedCommand(
        command: HarvestCircleCommand,
        hideChooser: Boolean = false,
    ) {
        launchCommand {
            when (val result = gateway.execute(command)) {
                is HarvestCircleCommandResult.Accepted -> {
                    retryableCommand = null
                    acceptSnapshot(result.receipt.snapshot)
                    mutableState.value =
                        mutableState.value.copy(
                            commandStatus = CommandStatus.ACCEPTED,
                            lastCommandRequestId = result.receipt.requestId,
                            lastFailureCode = null,
                            recoveryAction = WireRecoveryAction.NONE,
                        )
                    if (hideChooser) {
                        mutableState.value = mutableState.value.copy(accountChooserVisible = false)
                    }
                }
                is HarvestCircleCommandResult.Rejected -> {
                    retryableCommand =
                        command.takeIf {
                            result.failure.retryable && it !is HarvestCircleCommand.ImportAccount
                        }
                    mutableState.value =
                        mutableState.value.copy(
                            commandStatus =
                                if (result.failure.retryable) {
                                    CommandStatus.FAILED_RETRYABLE
                                } else {
                                    CommandStatus.FAILED_TERMINAL
                                },
                            lastCommandRequestId = result.failure.correlationId,
                            lastFailureCode = result.failure.code,
                            recoveryAction = result.failure.recoveryAction,
                            problem = result.failure.safeMessage,
                        )
                }
            }
        }
    }

    private fun launchCommand(operation: suspend () -> Unit) {
        if (rejectIfUnavailable()) return
        mutableState.value =
            mutableState.value.copy(
                busy = true,
                commandStatus = CommandStatus.RUNNING,
                problem = null,
            )
        command =
            scope.launch {
                try {
                    operation()
                    if (mutableState.value.commandStatus == CommandStatus.RUNNING) {
                        mutableState.value = mutableState.value.copy(commandStatus = CommandStatus.ACCEPTED)
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Exception) {
                    acceptFailure(error)
                } finally {
                    mutableState.value = mutableState.value.copy(busy = false)
                }
            }
    }

    private fun rejectIfUnavailable(): Boolean {
        if (closed) {
            mutableState.value =
                mutableState.value.copy(
                    commandStatus = CommandStatus.REJECTED_CLOSED,
                    problem = "The application runtime is closed.",
                )
            return true
        }
        if (command?.isActive == true) {
            mutableState.value =
                mutableState.value.copy(
                    commandStatus = CommandStatus.REJECTED_BUSY,
                    problem = "The application is busy. Try again.",
                )
            return true
        }
        if (mutableState.value.route !in setOf(HarvestCircleRoute.ACCOUNTS, HarvestCircleRoute.ACTIVE_ACCOUNT)) {
            mutableState.value =
                mutableState.value.copy(
                    commandStatus = CommandStatus.FAILED_TERMINAL,
                    problem = "The application runtime is not ready for this action.",
                )
            return true
        }
        return false
    }

    private fun rejectUnavailableIntent(message: String) {
        mutableState.value =
            mutableState.value.copy(
                commandStatus = if (closed) CommandStatus.REJECTED_CLOSED else CommandStatus.FAILED_TERMINAL,
                problem = message,
            )
    }

    private fun acceptSnapshot(snapshot: AppSnapshotDto) {
        if (snapshot.revision >= mutableState.value.snapshot.revision) {
            mutableState.value =
                mutableState.value.copy(
                    snapshot = snapshot,
                    route = snapshot.toHarvestCircleRoute(),
                )
        }
    }

    private fun acceptFailure(error: Throwable) {
        val native = error as? StudioException.Failure
        val gatewayFailure = (error as? HarvestCircleGatewayException)?.failure
        mutableState.value =
            mutableState.value.copy(
                busy = false,
                commandStatus =
                    if (native?.retryable == true || gatewayFailure?.retryable == true) {
                        CommandStatus.FAILED_RETRYABLE
                    } else {
                        CommandStatus.FAILED_TERMINAL
                    },
                lastCommandRequestId = gatewayFailure?.correlationId ?: native?.correlationId,
                lastFailureCode = gatewayFailure?.code ?: native?.code,
                recoveryAction =
                    gatewayFailure?.recoveryAction
                        ?: native?.recoveryAction
                        ?: WireRecoveryAction.NONE,
                problem = gatewayFailure?.safeMessage ?: native?.safeMessage ?: "The application command failed.",
            )
    }

    private fun releaseGeneratedRecovery(recovery: PendingGeneratedRecovery) {
        if (pendingGeneratedRecovery === recovery) {
            pendingGeneratedRecovery = null
        }
        recovery.backup.clear()
        recovery.ticket.close()
        mutableState.value = mutableState.value.copy(generatedKeyBackup = null)
    }

    override fun close() {
        if (closed) return
        closed = true
        command?.cancel()
        pendingRemoval?.close()
        pendingGeneratedRecovery?.let(::releaseGeneratedRecovery)
        subscription?.close()
        runCatching { gateway.shutdown() }
            .onSuccess { receipt ->
                mutableState.value =
                    mutableState.value.copy(
                        route = if (receipt.closed) HarvestCircleRoute.CLOSED else HarvestCircleRoute.FATAL,
                        busy = false,
                        problem = if (receipt.closed) null else "The application could not shut down safely.",
                    )
            }.onFailure { error ->
                acceptFailure(error)
                mutableState.value = mutableState.value.copy(route = HarvestCircleRoute.FATAL, busy = false)
            }
    }
}

private data class PendingGeneratedRecovery(
    val ticket: GeneratedRecoveryTicket,
    val backup: GeneratedKeyBackup,
)

internal fun AppSnapshotDto.toHarvestCircleRoute(): HarvestCircleRoute =
    when (lifecycle) {
        AppLifecycleDto.OPENING -> HarvestCircleRoute.OPENING
        AppLifecycleDto.COMPATIBILITY_CHECKING -> HarvestCircleRoute.CHECKING_COMPATIBILITY
        AppLifecycleDto.ACQUIRING_OWNERSHIP -> HarvestCircleRoute.ACQUIRING_OWNERSHIP
        AppLifecycleDto.MIGRATING -> HarvestCircleRoute.MIGRATING
        AppLifecycleDto.RECOVERING -> HarvestCircleRoute.RECOVERING
        AppLifecycleDto.READY -> if (activeAccount != null) HarvestCircleRoute.ACTIVE_ACCOUNT else HarvestCircleRoute.ACCOUNTS
        AppLifecycleDto.DEGRADED -> HarvestCircleRoute.DEGRADED
        AppLifecycleDto.BLOCKED -> HarvestCircleRoute.BLOCKED
        AppLifecycleDto.SHUTTING_DOWN -> HarvestCircleRoute.SHUTTING_DOWN
        AppLifecycleDto.CLOSED -> HarvestCircleRoute.CLOSED
        AppLifecycleDto.FATAL -> HarvestCircleRoute.FATAL
    }
