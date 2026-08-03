package org.radroots.studio.application

import androidx.compose.runtime.State
import androidx.compose.runtime.mutableStateOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import org.radroots.studio.ffi.AppSnapshotDto
import org.radroots.studio.ffi.AppLifecycleDto
import org.radroots.studio.ffi.StudioException
import org.radroots.studio.ffi.WireErrorCode
import org.radroots.studio.ffi.WireRecoveryAction

enum class StudioRoute {
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

data class StudioStoreState(
    val snapshot: AppSnapshotDto,
    val route: StudioRoute = snapshot.toStudioRoute(),
    val importDraft: String = "",
    val generatedKeyBackup: GeneratedKeyBackup? = null,
    val pendingRemovalPublicKeyHex: String? = null,
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

class StudioAppStore(
    private val gateway: StudioCoreGateway,
    private val scope: CoroutineScope,
) : AutoCloseable {
    private val generatedRecovery = GeneratedRecoveryController()
    private val mutableState = mutableStateOf(StudioStoreState(snapshot = gateway.snapshot()))
    private var closed = false
    private var subscription: AutoCloseable? = null
    private var pendingRemoval: RemovalTicket? = null
    private var pendingGeneratedRecovery: GeneratedRecoveryTicket? = null
    private var command: Job? = null

    val state: State<StudioStoreState>
        get() = mutableState

    init {
        launchCommand {
            val registered = gateway.subscribeChanges { change ->
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
        mutableState.value = mutableState.value.copy(
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
        mutableState.value = mutableState.value.copy(
            accountEntryMode = AccountEntryMode.CHOICE,
            importDraft = "",
            problem = null,
        )
    }

    fun generateAccount() {
        launchCommand {
            val recovery = gateway.beginGeneratedAccount()
            pendingGeneratedRecovery = recovery
            mutableState.value = mutableState.value.copy(
                generatedKeyBackup = generatedRecovery.begin(
                    recovery.account.npub,
                    recovery.takeRecoveryNsec(),
                ),
            )
        }
    }

    fun acknowledgeGeneratedKeyBackup() {
        val recovery = pendingGeneratedRecovery ?: run {
            rejectUnavailableIntent("Generated-key recovery is not available.")
            return
        }
        pendingGeneratedRecovery = null
        runSnapshotCommand {
            try {
                recovery.acknowledge().also {
                    generatedRecovery.acknowledge()
                    mutableState.value = mutableState.value.copy(generatedKeyBackup = null)
                }
            } finally {
                recovery.close()
            }
        }
    }

    fun cancelGeneratedKeyBackup() {
        val recovery = pendingGeneratedRecovery ?: run {
            rejectUnavailableIntent("Generated-key recovery is not available.")
            return
        }
        pendingGeneratedRecovery = null
        launchCommand {
            try {
                recovery.cancel()
                generatedRecovery.acknowledge()
                mutableState.value = mutableState.value.copy(generatedKeyBackup = null)
            } finally {
                recovery.close()
            }
        }
    }

    fun importSecretKey() {
        if (rejectIfUnavailable()) return
        val input = mutableState.value.importDraft.encodeToByteArray()
        mutableState.value = mutableState.value.copy(importDraft = "")
        runTypedCommand(StudioCommand.ImportAccount(input))
    }

    fun selectAccount(publicKeyHex: String) {
        runTypedCommand(StudioCommand.SelectAccount(publicKeyHex))
    }

    fun activateAccount(publicKeyHex: String) {
        runTypedCommand(StudioCommand.ActivateAccount(publicKeyHex), hideChooser = true)
    }

    fun signOut() {
        runTypedCommand(StudioCommand.SignOut, hideChooser = true)
    }

    fun showAccountChooser() {
        mutableState.value = mutableState.value.copy(accountChooserVisible = true, problem = null)
    }

    fun hideAccountChooser() {
        mutableState.value = mutableState.value.copy(accountChooserVisible = false)
    }

    fun refreshActiveProfile() {
        runTypedCommand(StudioCommand.RefreshProfile)
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
                mutableState.value = mutableState.value.copy(
                    pendingRemovalPublicKeyHex = publicKeyHex,
                )
            }.getOrThrow()
        }
    }

    fun cancelAccountRemoval() {
        pendingRemoval?.close()
        pendingRemoval = null
        mutableState.value = mutableState.value.copy(pendingRemovalPublicKeyHex = null)
    }

    fun confirmAccountRemoval() {
        val ticket = pendingRemoval ?: run {
            rejectUnavailableIntent("Account removal confirmation is not available.")
            return
        }
        pendingRemoval = null
        runSnapshotCommand {
            try {
                gateway.confirmAccountRemoval(ticket).also {
                    mutableState.value = mutableState.value.copy(pendingRemovalPublicKeyHex = null)
                }
            } finally {
                ticket.close()
                mutableState.value = mutableState.value.copy(pendingRemovalPublicKeyHex = null)
            }
        }
    }

    fun dismissProblem() {
        mutableState.value = mutableState.value.copy(problem = null)
    }

    private fun runSnapshotCommand(operation: suspend () -> AppSnapshotDto) {
        launchCommand { acceptSnapshot(operation()) }
    }

    private fun runTypedCommand(command: StudioCommand, hideChooser: Boolean = false) {
        launchCommand {
            when (val result = gateway.execute(command)) {
                is StudioCommandResult.Accepted -> {
                    acceptSnapshot(result.receipt.snapshot)
                    mutableState.value = mutableState.value.copy(
                        commandStatus = CommandStatus.ACCEPTED,
                        lastCommandRequestId = result.receipt.requestId,
                        lastFailureCode = null,
                        recoveryAction = WireRecoveryAction.NONE,
                    )
                    if (hideChooser) {
                        mutableState.value = mutableState.value.copy(accountChooserVisible = false)
                    }
                }
                is StudioCommandResult.Rejected -> {
                    mutableState.value = mutableState.value.copy(
                        commandStatus = if (result.failure.retryable) {
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
        mutableState.value = mutableState.value.copy(
            busy = true,
            commandStatus = CommandStatus.RUNNING,
            problem = null,
        )
        command = scope.launch {
            try {
                operation()
                if (mutableState.value.commandStatus == CommandStatus.RUNNING) {
                    mutableState.value = mutableState.value.copy(commandStatus = CommandStatus.ACCEPTED)
                }
            } catch (error: Throwable) {
                acceptFailure(error)
            } finally {
                mutableState.value = mutableState.value.copy(busy = false)
            }
        }
    }

    private fun rejectIfUnavailable(): Boolean {
        if (closed) {
            mutableState.value = mutableState.value.copy(
                commandStatus = CommandStatus.REJECTED_CLOSED,
                problem = "The application runtime is closed.",
            )
            return true
        }
        if (command?.isActive == true) {
            mutableState.value = mutableState.value.copy(
                commandStatus = CommandStatus.REJECTED_BUSY,
                problem = "The application is busy. Try again.",
            )
            return true
        }
        if (mutableState.value.route !in setOf(StudioRoute.ACCOUNTS, StudioRoute.ACTIVE_ACCOUNT)) {
            mutableState.value = mutableState.value.copy(
                commandStatus = CommandStatus.FAILED_TERMINAL,
                problem = "The application runtime is not ready for this action.",
            )
            return true
        }
        return false
    }

    private fun rejectUnavailableIntent(message: String) {
        mutableState.value = mutableState.value.copy(
            commandStatus = if (closed) CommandStatus.REJECTED_CLOSED else CommandStatus.FAILED_TERMINAL,
            problem = message,
        )
    }

    private fun acceptSnapshot(snapshot: AppSnapshotDto) {
        if (snapshot.revision >= mutableState.value.snapshot.revision) {
            mutableState.value = mutableState.value.copy(
                snapshot = snapshot,
                route = snapshot.toStudioRoute(),
            )
        }
    }

    private fun acceptFailure(error: Throwable) {
        val native = error as? StudioException.Failure
        mutableState.value = mutableState.value.copy(
            busy = false,
            commandStatus = if (native?.retryable == true) {
                CommandStatus.FAILED_RETRYABLE
            } else {
                CommandStatus.FAILED_TERMINAL
            },
            lastCommandRequestId = native?.correlationId,
            lastFailureCode = native?.code,
            recoveryAction = native?.recoveryAction ?: WireRecoveryAction.NONE,
            problem = native?.safeMessage ?: "The application command failed.",
        )
    }

    override fun close() {
        if (closed) return
        closed = true
        command?.cancel()
        pendingRemoval?.close()
        pendingGeneratedRecovery?.close()
        subscription?.close()
        generatedRecovery.close()
        runCatching { gateway.shutdown() }
            .onSuccess { receipt ->
                mutableState.value = mutableState.value.copy(
                    route = if (receipt.closed) StudioRoute.CLOSED else StudioRoute.FATAL,
                    busy = false,
                    problem = if (receipt.closed) null else "The application could not shut down safely.",
                )
            }
            .onFailure { error ->
                acceptFailure(error)
                mutableState.value = mutableState.value.copy(route = StudioRoute.FATAL, busy = false)
            }
    }
}

internal fun AppSnapshotDto.toStudioRoute(): StudioRoute = when (lifecycle) {
    AppLifecycleDto.OPENING -> StudioRoute.OPENING
    AppLifecycleDto.COMPATIBILITY_CHECKING -> StudioRoute.CHECKING_COMPATIBILITY
    AppLifecycleDto.ACQUIRING_OWNERSHIP -> StudioRoute.ACQUIRING_OWNERSHIP
    AppLifecycleDto.MIGRATING -> StudioRoute.MIGRATING
    AppLifecycleDto.RECOVERING -> StudioRoute.RECOVERING
    AppLifecycleDto.READY -> if (activeAccount != null) StudioRoute.ACTIVE_ACCOUNT else StudioRoute.ACCOUNTS
    AppLifecycleDto.DEGRADED -> StudioRoute.DEGRADED
    AppLifecycleDto.BLOCKED -> StudioRoute.BLOCKED
    AppLifecycleDto.SHUTTING_DOWN -> StudioRoute.SHUTTING_DOWN
    AppLifecycleDto.CLOSED -> StudioRoute.CLOSED
    AppLifecycleDto.FATAL -> StudioRoute.FATAL
}
