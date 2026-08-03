package org.radroots.studio.application

import androidx.compose.runtime.State
import androidx.compose.runtime.mutableStateOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import org.radroots.studio.ffi.AppSnapshotDto
import org.radroots.studio.ffi.AppLifecycleDto
import org.radroots.studio.ffi.StudioException

enum class StudioRoute {
    BOOTING,
    ACCOUNTS,
    ACTIVE_ACCOUNT,
    FATAL,
    CLOSED,
}

data class StudioStoreState(
    val snapshot: AppSnapshotDto,
    val route: StudioRoute = snapshot.toStudioRoute(),
    val importDraft: String = "",
    val generatedKeyBackup: GeneratedKeyBackup? = null,
    val pendingRemovalPublicKeyHex: String? = null,
    val accountChooserVisible: Boolean = false,
    val busy: Boolean = false,
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
        val recovery = pendingGeneratedRecovery ?: return
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

    fun importSecretKey() {
        if (command?.isActive == true) return
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
        pendingGeneratedRecovery?.close()
        pendingRemoval = null
        mutableState.value = mutableState.value.copy(pendingRemovalPublicKeyHex = null)
    }

    fun confirmAccountRemoval() {
        val ticket = pendingRemoval ?: return
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
                    if (hideChooser) {
                        mutableState.value = mutableState.value.copy(accountChooserVisible = false)
                    }
                }
                is StudioCommandResult.Rejected -> {
                    mutableState.value = mutableState.value.copy(problem = result.failure.safeMessage)
                }
            }
        }
    }

    private fun launchCommand(operation: suspend () -> Unit) {
        if (closed || command?.isActive == true) return
        mutableState.value = mutableState.value.copy(busy = true, problem = null)
        command = scope.launch {
            runCatching { operation() }
                .onFailure(::acceptFailure)
            mutableState.value = mutableState.value.copy(busy = false)
        }
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
        val message = (error as? StudioException.Failure)?.safeMessage
            ?: "The application command failed."
        mutableState.value = mutableState.value.copy(busy = false, problem = message)
    }

    override fun close() {
        if (closed) return
        closed = true
        command?.cancel()
        pendingRemoval?.close()
        subscription?.close()
        generatedRecovery.close()
        gateway.close()
        mutableState.value = mutableState.value.copy(route = StudioRoute.CLOSED, busy = false)
    }
}

private fun AppSnapshotDto.toStudioRoute(): StudioRoute = when {
    lifecycle == AppLifecycleDto.BOOTING -> StudioRoute.BOOTING
    lifecycle == AppLifecycleDto.FATAL -> StudioRoute.FATAL
    activeAccount != null -> StudioRoute.ACTIVE_ACCOUNT
    else -> StudioRoute.ACCOUNTS
}
