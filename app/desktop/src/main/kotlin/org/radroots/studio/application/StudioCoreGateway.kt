package org.radroots.studio.application

import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.runBlocking
import org.radroots.studio.ffi.AppSnapshotDto
import org.radroots.studio.ffi.AccountDto
import org.radroots.studio.ffi.GeneratedRecoveryRequest
import org.radroots.studio.ffi.ObserverSubscription
import org.radroots.studio.ffi.RemovalRequest
import org.radroots.studio.ffi.RequestContextDto
import org.radroots.studio.ffi.SnapshotChangeDto
import org.radroots.studio.ffi.StudioAppCore
import org.radroots.studio.ffi.StudioChangeObserver
import org.radroots.studio.ffi.StudioException
import org.radroots.studio.ffi.WireErrorCategory
import org.radroots.studio.ffi.WireErrorCode
import org.radroots.studio.ffi.WireRecoveryAction

interface RemovalTicket : AutoCloseable

interface GeneratedRecoveryTicket : AutoCloseable {
    val account: AccountDto

    fun takeRecoveryNsec(): String

    suspend fun acknowledge(): AppSnapshotDto

    suspend fun cancel(): Boolean
}

data class StudioChange(
    val snapshot: AppSnapshotDto,
    val previousRevision: ULong?,
)

sealed interface StudioCommand {
    data class ImportAccount(val bytes: ByteArray) : StudioCommand
    data class SelectAccount(val publicKeyHex: String) : StudioCommand
    data class ActivateAccount(val publicKeyHex: String) : StudioCommand
    data object SignOut : StudioCommand
    data object RefreshProfile : StudioCommand
}

data class StudioCommandReceipt(
    val requestId: String,
    val committedRevision: ULong,
    val snapshot: AppSnapshotDto,
)

data class StudioCommandFailure(
    val code: WireErrorCode,
    val category: WireErrorCategory,
    val retryable: Boolean,
    val recoveryAction: WireRecoveryAction,
    val correlationId: String?,
    val safeMessage: String,
)

data class StudioShutdownReceipt(
    val finalRevision: ULong,
    val closed: Boolean,
)

sealed interface StudioCommandResult {
    data class Accepted(val receipt: StudioCommandReceipt) : StudioCommandResult
    data class Rejected(val failure: StudioCommandFailure) : StudioCommandResult
}

interface StudioCoreGateway : AutoCloseable {
    fun snapshot(): AppSnapshotDto

    suspend fun subscribeChanges(onChange: (StudioChange) -> Unit): AutoCloseable

    suspend fun execute(command: StudioCommand): StudioCommandResult

    suspend fun bootstrap(): AppSnapshotDto

    suspend fun beginGeneratedAccount(): GeneratedRecoveryTicket

    suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket

    suspend fun confirmAccountRemoval(ticket: RemovalTicket): AppSnapshotDto

    fun shutdown(): StudioShutdownReceipt
}

class NativeStudioCoreGateway(
    private val core: StudioAppCore,
) : StudioCoreGateway {
    private val nextRequest = AtomicLong(1)
    private val shutdownLock = Any()
    private var shutdownReceipt: StudioShutdownReceipt? = null
    override fun snapshot(): AppSnapshotDto = core.snapshot()

    override suspend fun subscribeChanges(onChange: (StudioChange) -> Unit): AutoCloseable {
        val subscription = core.subscribeChangesV2(
            object : StudioChangeObserver {
                override fun onChange(change: SnapshotChangeDto) {
                    onChange(StudioChange(change.snapshot, change.previousRevision))
                }
            },
        )
        return NativeSubscription(subscription)
    }

    override suspend fun execute(command: StudioCommand): StudioCommandResult {
        val context = requestContext()
        return try {
            val snapshot = when (command) {
                is StudioCommand.ImportAccount -> try {
                    core.importAccountV2(context, command.bytes).snapshot
                } finally {
                    command.bytes.fill(0)
                }
                is StudioCommand.SelectAccount -> core.selectAccount(command.publicKeyHex)
                is StudioCommand.ActivateAccount -> core.activateAccount(command.publicKeyHex)
                StudioCommand.SignOut -> core.signOut()
                StudioCommand.RefreshProfile -> core.refreshActiveProfile()
            }
            StudioCommandResult.Accepted(
                StudioCommandReceipt(context.requestId, snapshot.revision, snapshot),
            )
        } catch (error: Throwable) {
            StudioCommandResult.Rejected(error.toStudioCommandFailure(context.requestId))
        }
    }

    override suspend fun bootstrap(): AppSnapshotDto = core.bootstrap()

    override suspend fun beginGeneratedAccount(): GeneratedRecoveryTicket =
        NativeGeneratedRecoveryTicket(core, core.beginGeneratedAccountV2())

    override suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket =
        NativeRemovalTicket(core.requestAccountRemoval(publicKeyHex))

    override suspend fun confirmAccountRemoval(ticket: RemovalTicket): AppSnapshotDto {
        require(ticket is NativeRemovalTicket) { "Removal ticket does not belong to native core" }
        return core.confirmAccountRemoval(ticket.request)
    }

    override fun shutdown(): StudioShutdownReceipt = synchronized(shutdownLock) {
        shutdownReceipt ?: run {
            val receipt = runBlocking { core.shutdownV2() }
            StudioShutdownReceipt(receipt.finalRevision, receipt.closed).also {
                check(it.closed) { "Native runtime returned an incomplete shutdown receipt" }
                shutdownReceipt = it
                core.close()
            }
        }
    }

    override fun close() {
        shutdown()
    }

    private fun requestContext(): RequestContextDto = RequestContextDto(
        requestId = "kotlin:${nextRequest.getAndIncrement()}",
        expectedRevision = core.snapshot().revision,
        deadlineMillis = 30_000UL,
    )
}

internal fun Throwable.toStudioCommandFailure(fallbackCorrelationId: String): StudioCommandFailure {
    val native = this as? StudioException.Failure
    return StudioCommandFailure(
        code = native?.code ?: WireErrorCode.INTERNAL,
        category = native?.category ?: WireErrorCategory.INTERNAL,
        retryable = native?.retryable ?: false,
        recoveryAction = native?.recoveryAction ?: WireRecoveryAction.NONE,
        correlationId = native?.correlationId ?: fallbackCorrelationId,
        safeMessage = native?.safeMessage ?: "The application command failed.",
    )
}

private class NativeSubscription(
    private val subscription: ObserverSubscription,
) : AutoCloseable {
    override fun close() {
        subscription.unsubscribe()
        subscription.close()
    }
}

private class NativeRemovalTicket(
    val request: RemovalRequest,
) : RemovalTicket {
    override fun close() {
        request.close()
    }
}

private class NativeGeneratedRecoveryTicket(
    private val core: StudioAppCore,
    private val request: GeneratedRecoveryRequest,
) : GeneratedRecoveryTicket {
    override val account: AccountDto = request.account()

    override fun takeRecoveryNsec(): String = request.takeRecoveryNsec()

    override suspend fun acknowledge(): AppSnapshotDto =
        core.acknowledgeGeneratedAccountV2(request)

    override suspend fun cancel(): Boolean = core.cancelGeneratedAccountV2(request)

    override fun close() {
        runBlocking { runCatching { cancel() } }
        request.close()
    }
}
