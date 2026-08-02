package org.radroots.studio.application

import androidx.compose.runtime.State
import androidx.compose.runtime.mutableStateOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import org.radroots.studio.ffi.AppSnapshotDto
import org.radroots.studio.ffi.StudioException

data class GeneratedKeyBackup(
    val npub: String,
    val nsec: String,
)

data class StudioStoreState(
    val snapshot: AppSnapshotDto,
    val importDraft: String = "",
    val generatedKeyBackup: GeneratedKeyBackup? = null,
    val pendingRemovalPublicKeyHex: String? = null,
    val accountChooserVisible: Boolean = false,
    val busy: Boolean = false,
    val problem: String? = null,
)

class StudioAppStore(
    private val gateway: StudioCoreGateway,
    private val scope: CoroutineScope,
) : AutoCloseable {
    private val mutableState = mutableStateOf(StudioStoreState(snapshot = gateway.snapshot()))
    private var closed = false
    private val subscription = gateway.subscribe { snapshot ->
        scope.launch {
            if (!closed) acceptSnapshot(snapshot)
        }
    }
    private var pendingRemoval: RemovalTicket? = null
    private var command: Job? = null

    val state: State<StudioStoreState>
        get() = mutableState

    init {
        runCommand { gateway.bootstrap() }
    }

    fun editImportDraft(value: String) {
        mutableState.value = mutableState.value.copy(importDraft = value, problem = null)
    }

    fun generateAccount() {
        runCommand {
            val receipt = gateway.generateAccount()
            mutableState.value = mutableState.value.copy(
                generatedKeyBackup = GeneratedKeyBackup(receipt.account.npub, receipt.nsec),
            )
            receipt.snapshot
        }
    }

    fun acknowledgeGeneratedKeyBackup() {
        mutableState.value = mutableState.value.copy(generatedKeyBackup = null)
    }

    fun importSecretKey() {
        if (command?.isActive == true) return
        val input = mutableState.value.importDraft
        mutableState.value = mutableState.value.copy(importDraft = "")
        runCommand { gateway.importSecretKey(input) }
    }

    fun selectAccount(publicKeyHex: String) {
        runCommand { gateway.selectAccount(publicKeyHex) }
    }

    fun activateAccount(publicKeyHex: String) {
        runCommand {
            gateway.activateAccount(publicKeyHex).also {
                mutableState.value = mutableState.value.copy(accountChooserVisible = false)
            }
        }
    }

    fun signOut() {
        runCommand {
            gateway.signOut().also {
                mutableState.value = mutableState.value.copy(accountChooserVisible = false)
            }
        }
    }

    fun showAccountChooser() {
        mutableState.value = mutableState.value.copy(accountChooserVisible = true, problem = null)
    }

    fun hideAccountChooser() {
        mutableState.value = mutableState.value.copy(accountChooserVisible = false)
    }

    fun refreshActiveProfile() {
        runCommand { gateway.refreshActiveProfile() }
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
        val ticket = pendingRemoval ?: return
        pendingRemoval = null
        runCommand {
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

    private fun runCommand(operation: suspend () -> AppSnapshotDto) {
        launchCommand { acceptSnapshot(operation()) }
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
            mutableState.value = mutableState.value.copy(snapshot = snapshot)
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
        subscription.close()
        gateway.close()
    }
}
