package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import org.harvestcircle.ffi.AppSnapshotDto
import org.harvestcircle.ffi.GeneratedRecoveryRequest
import org.harvestcircle.ffi.HarvestCircleAppCore
import org.harvestcircle.ffi.HarvestCircleChangeObserver
import org.harvestcircle.ffi.HarvestCircleException
import org.harvestcircle.ffi.IdentityDto
import org.harvestcircle.ffi.ObserverSubscription
import org.harvestcircle.ffi.RemovalRequest
import org.harvestcircle.ffi.RequestContextDto
import org.harvestcircle.ffi.SnapshotChangeDto
import org.harvestcircle.ffi.WireErrorCategory
import org.harvestcircle.ffi.WireErrorCode
import org.harvestcircle.ffi.WireRecoveryAction
import java.util.concurrent.atomic.AtomicLong

interface RemovalTicket : AutoCloseable {
    val publicKeyHex: String
    val deletesLocalCredential: Boolean
    val signsOut: Boolean
    val expiresAtSeconds: Long
}

interface GeneratedRecoveryTicket : AutoCloseable {
    val requestId: String
    val identity: IdentityDto

    fun takeRecoveryNsec(): String

    suspend fun acknowledge(): AppSnapshotDto

    suspend fun cancel(): Boolean
}

data class HarvestCircleChange(
    val snapshot: AppSnapshotDto,
    val previousRevision: ULong?,
)

sealed interface HarvestCircleCommand {
    data class ImportIdentity(
        val bytes: ByteArray,
    ) : HarvestCircleCommand

    data class SelectIdentity(
        val publicKeyHex: String,
    ) : HarvestCircleCommand

    data class ActivateIdentity(
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

    suspend fun beginGeneratedIdentity(): GeneratedRecoveryTicket

    suspend fun requestIdentityRemoval(publicKeyHex: String): RemovalTicket

    suspend fun confirmIdentityRemoval(ticket: RemovalTicket): AppSnapshotDto

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
                    is HarvestCircleCommand.ImportIdentity ->
                        try {
                            core.importIdentity(context, command.bytes).snapshot
                        } finally {
                            command.bytes.fill(0)
                        }
                    is HarvestCircleCommand.SelectIdentity -> core.selectIdentity(command.publicKeyHex)
                    is HarvestCircleCommand.ActivateIdentity -> core.activateIdentity(command.publicKeyHex)
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

    override suspend fun beginGeneratedIdentity(): GeneratedRecoveryTicket {
        val requestId = nextRequestId()
        return try {
            val request = core.beginGeneratedIdentity()
            try {
                NativeGeneratedRecoveryTicket(core, request, ::requestContext, requestId, request.identity())
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

    override suspend fun requestIdentityRemoval(publicKeyHex: String): RemovalTicket =
        NativeRemovalTicket(core.requestIdentityRemoval(publicKeyHex))

    override suspend fun confirmIdentityRemoval(ticket: RemovalTicket): AppSnapshotDto {
        require(ticket is NativeRemovalTicket) { "Removal ticket does not belong to native core" }
        return core.confirmIdentityRemoval(requestContext(), ticket.request)
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
    override val identity: IdentityDto,
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
        return call("The generated identity could not be saved.", context.requestId) {
            core.acknowledgeGeneratedIdentity(context, request)
        }
    }

    override suspend fun cancel(): Boolean =
        call("The generated key could not be cancelled safely.") {
            core.cancelGeneratedIdentity(request)
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
