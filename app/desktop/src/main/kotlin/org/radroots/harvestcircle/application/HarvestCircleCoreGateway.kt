package org.radroots.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import org.radroots.harvestcircle.ffi.AccountDto
import org.radroots.harvestcircle.ffi.AppSnapshotDto
import org.radroots.harvestcircle.ffi.GeneratedRecoveryRequest
import org.radroots.harvestcircle.ffi.HarvestCircleAppCore
import org.radroots.harvestcircle.ffi.HarvestCircleChangeObserver
import org.radroots.harvestcircle.ffi.HarvestCircleException
import org.radroots.harvestcircle.ffi.ObserverSubscription
import org.radroots.harvestcircle.ffi.RemovalRequest
import org.radroots.harvestcircle.ffi.RequestContextDto
import org.radroots.harvestcircle.ffi.SnapshotChangeDto
import org.radroots.harvestcircle.ffi.WireErrorCategory
import org.radroots.harvestcircle.ffi.WireErrorCode
import org.radroots.harvestcircle.ffi.WireRecoveryAction
import java.util.concurrent.atomic.AtomicLong

interface RemovalTicket : AutoCloseable {
    val publicKeyHex: String
    val deletesLocalCredential: Boolean
    val signsOut: Boolean
    val expiresAtSeconds: Long
}

interface GeneratedRecoveryTicket : AutoCloseable {
    val requestId: String
    val account: AccountDto

    fun takeRecoveryNsec(): String

    suspend fun acknowledge(): AppSnapshotDto

    suspend fun cancel(): Boolean
}

data class HarvestCircleChange(
    val snapshot: AppSnapshotDto,
    val previousRevision: ULong?,
)

sealed interface HarvestCircleCommand {
    data class ImportAccount(
        val bytes: ByteArray,
    ) : HarvestCircleCommand

    data class SelectAccount(
        val publicKeyHex: String,
    ) : HarvestCircleCommand

    data class ActivateAccount(
        val publicKeyHex: String,
    ) : HarvestCircleCommand

    data object SignOut : HarvestCircleCommand

    data object RefreshProfile : HarvestCircleCommand
}

data class HarvestCircleCommandReceipt(
    val requestId: String,
    val committedRevision: ULong,
    val snapshot: AppSnapshotDto,
)

data class HarvestCircleCommandFailure(
    val code: WireErrorCode,
    val category: WireErrorCategory,
    val retryable: Boolean,
    val recoveryAction: WireRecoveryAction,
    val correlationId: String?,
    val safeMessage: String,
)

data class HarvestCircleShutdownReceipt(
    val finalRevision: ULong,
    val closed: Boolean,
)

sealed interface HarvestCircleCommandResult {
    data class Accepted(
        val receipt: HarvestCircleCommandReceipt,
    ) : HarvestCircleCommandResult

    data class Rejected(
        val failure: HarvestCircleCommandFailure,
    ) : HarvestCircleCommandResult
}

interface HarvestCircleCoreGateway : AutoCloseable {
    fun snapshot(): AppSnapshotDto

    suspend fun subscribeChanges(onChange: (HarvestCircleChange) -> Unit): AutoCloseable

    suspend fun execute(command: HarvestCircleCommand): HarvestCircleCommandResult

    suspend fun bootstrap(): AppSnapshotDto

    suspend fun beginGeneratedAccount(): GeneratedRecoveryTicket

    suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket

    suspend fun confirmAccountRemoval(ticket: RemovalTicket): AppSnapshotDto

    fun shutdown(): HarvestCircleShutdownReceipt
}

class NativeHarvestCircleCoreGateway(
    private val core: HarvestCircleAppCore,
) : HarvestCircleCoreGateway {
    private val nextRequest = AtomicLong(1)
    private val shutdownLock = Any()
    private var shutdownReceipt: HarvestCircleShutdownReceipt? = null

    override fun snapshot(): AppSnapshotDto = core.snapshot()

    override suspend fun subscribeChanges(onChange: (HarvestCircleChange) -> Unit): AutoCloseable {
        val subscription =
            core.subscribeChangesV2(
                object : HarvestCircleChangeObserver {
                    override fun onChange(change: SnapshotChangeDto) {
                        onChange(HarvestCircleChange(change.snapshot, change.previousRevision))
                    }
                },
            )
        return NativeSubscription(subscription)
    }

    override suspend fun execute(command: HarvestCircleCommand): HarvestCircleCommandResult {
        val context = requestContext()
        return try {
            val snapshot =
                when (command) {
                    is HarvestCircleCommand.ImportAccount ->
                        try {
                            core.importAccountV2(context, command.bytes).snapshot
                        } finally {
                            command.bytes.fill(0)
                        }
                    is HarvestCircleCommand.SelectAccount -> core.selectAccount(command.publicKeyHex)
                    is HarvestCircleCommand.ActivateAccount -> core.activateAccount(command.publicKeyHex)
                    HarvestCircleCommand.SignOut -> core.signOut()
                    HarvestCircleCommand.RefreshProfile -> core.refreshActiveProfile()
                }
            HarvestCircleCommandResult.Accepted(
                HarvestCircleCommandReceipt(context.requestId, snapshot.revision, snapshot),
            )
        } catch (error: CancellationException) {
            throw error
        } catch (error: Exception) {
            HarvestCircleCommandResult.Rejected(error.toHarvestCircleCommandFailure(context.requestId))
        }
    }

    override suspend fun bootstrap(): AppSnapshotDto = core.bootstrap()

    override suspend fun beginGeneratedAccount(): GeneratedRecoveryTicket {
        val requestId = nextRequestId()
        return try {
            val request = core.beginGeneratedAccountV2()
            try {
                NativeGeneratedRecoveryTicket(core, request, ::requestContext, requestId, request.account())
            } catch (error: Exception) {
                request.close()
                throw error
            }
        } catch (error: CancellationException) {
            throw error
        } catch (error: Exception) {
            throw HarvestCircleGatewayException(
                error.toHarvestCircleCommandFailure(
                    requestId,
                    "The generated key could not be prepared.",
                ),
            )
        }
    }

    override suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket =
        NativeRemovalTicket(core.requestAccountRemoval(publicKeyHex))

    override suspend fun confirmAccountRemoval(ticket: RemovalTicket): AppSnapshotDto {
        require(ticket is NativeRemovalTicket) { "Removal ticket does not belong to native core" }
        return core.confirmAccountRemoval(requestContext(), ticket.request)
    }

    override fun shutdown(): HarvestCircleShutdownReceipt =
        synchronized(shutdownLock) {
            shutdownReceipt ?: run {
                val receipt = runBlocking { core.shutdownV2() }
                HarvestCircleShutdownReceipt(receipt.finalRevision, receipt.closed).also {
                    check(it.closed) { "Native runtime returned an incomplete shutdown receipt" }
                    shutdownReceipt = it
                    core.close()
                }
            }
        }

    override fun close() {
        shutdown()
    }

    private fun requestContext(): RequestContextDto =
        RequestContextDto(
            requestId = nextRequestId(),
            expectedRevision = core.snapshot().revision,
            deadlineMillis = 30_000UL,
        )

    private fun nextRequestId(): String = "kotlin:${nextRequest.getAndIncrement()}"
}

internal class HarvestCircleGatewayException(
    val failure: HarvestCircleCommandFailure,
) : Exception(failure.safeMessage)

internal fun Throwable.toHarvestCircleCommandFailure(
    fallbackCorrelationId: String,
    fallbackSafeMessage: String = "The application command failed.",
): HarvestCircleCommandFailure {
    val native = this as? HarvestCircleException.Failure
    return HarvestCircleCommandFailure(
        code = native?.code ?: WireErrorCode.INTERNAL,
        category = native?.category ?: WireErrorCategory.INTERNAL,
        retryable = native?.retryable ?: false,
        recoveryAction = native?.recoveryAction ?: WireRecoveryAction.NONE,
        correlationId = native?.correlationId ?: fallbackCorrelationId,
        safeMessage = native?.safeMessage ?: fallbackSafeMessage,
    )
}

private class NativeSubscription(
    private val subscription: ObserverSubscription,
) : AutoCloseable {
    override fun close() {
        try {
            runBlocking { subscription.unsubscribe() }
        } finally {
            subscription.close()
        }
    }
}

private class NativeRemovalTicket(
    val request: RemovalRequest,
) : RemovalTicket {
    override val publicKeyHex: String = request.publicKeyHex()
    override val deletesLocalCredential: Boolean = request.deletesLocalCredential()
    override val signsOut: Boolean = request.signsOut()
    override val expiresAtSeconds: Long = request.expiresAtSeconds()

    override fun close() {
        request.close()
    }
}

private class NativeGeneratedRecoveryTicket(
    private val core: HarvestCircleAppCore,
    private val request: GeneratedRecoveryRequest,
    private val requestContext: () -> RequestContextDto,
    override val requestId: String,
    override val account: AccountDto,
) : GeneratedRecoveryTicket {
    override fun takeRecoveryNsec(): String =
        try {
            request.takeRecoveryNsec()
        } catch (error: Exception) {
            throw HarvestCircleGatewayException(
                error.toHarvestCircleCommandFailure(
                    requestId,
                    "The generated recovery key could not be read.",
                ),
            )
        }

    override suspend fun acknowledge(): AppSnapshotDto {
        val context = requestContext()
        return call("The generated account could not be saved.", context.requestId) {
            core.acknowledgeGeneratedAccountV2(context, request)
        }
    }

    override suspend fun cancel(): Boolean =
        call("The generated key could not be cancelled safely.") {
            core.cancelGeneratedAccountV2(request)
        }

    override fun close() {
        request.close()
    }

    private suspend fun <T> call(
        fallbackSafeMessage: String,
        correlationId: String = requestId,
        operation: suspend () -> T,
    ): T =
        try {
            operation()
        } catch (error: CancellationException) {
            throw error
        } catch (error: Exception) {
            throw HarvestCircleGatewayException(
                error.toHarvestCircleCommandFailure(correlationId, fallbackSafeMessage),
            )
        }
}
