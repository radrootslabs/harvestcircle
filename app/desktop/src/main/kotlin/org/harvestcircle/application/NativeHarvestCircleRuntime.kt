package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.harvestcircle.ffi.AccountCommandReceiptDto
import org.harvestcircle.ffi.AccountDto
import org.harvestcircle.ffi.AppSnapshotDto
import org.harvestcircle.ffi.GeneratedRecoveryRequest
import org.harvestcircle.ffi.HarvestCircleAppCore
import org.harvestcircle.ffi.HarvestCircleChangeObserver
import org.harvestcircle.ffi.ObserverSubscription
import org.harvestcircle.ffi.RemovalRequest
import org.harvestcircle.ffi.RequestContextDto
import org.harvestcircle.ffi.ShutdownReceiptDto
import org.harvestcircle.ffi.SnapshotChangeDto
import org.harvestcircle.ffi.compatibilityDescriptor
import java.util.concurrent.atomic.AtomicLong

class HarvestCircleRuntimeException(
    val problem: ApplicationProblem,
) : Exception(problem.safeMessage)

class NativeHarvestCircleRuntime internal constructor(
    private val native: NativeCorePort,
    private val handleIds: NativeHandleIdSource = AtomicNativeHandleIdSource(),
) : HarvestCircleRuntime {
    private val recoveryMutex = Mutex()
    private val recoveryHandles = mutableMapOf<RecoveryRequestId, NativeGeneratedRecoveryHandle>()
    private val removalMutex = Mutex()
    private val removalHandles = mutableMapOf<RemovalRequestId, NativeRemovalHandle>()
    private val shutdownMutex = Mutex()
    private var shutdownReceipt: ShutdownReceipt? = null

    override suspend fun bootstrap(): ApplicationSnapshot = callNative { native.bootstrap().toApplicationSnapshot() }

    override fun currentSnapshot(): ApplicationSnapshot =
        try {
            native.snapshot().toApplicationSnapshot()
        } catch (error: HarvestCircleRuntimeException) {
            throw error
        } catch (error: Exception) {
            throw HarvestCircleRuntimeException(error.toApplicationProblem())
        }

    override fun changes(): Flow<ApplicationChange> =
        callbackFlow {
            val subscription =
                callNative {
                    native.subscribe { change ->
                        trySend(change.toApplicationChange())
                    }
                }
            awaitClose(subscription::close)
        }

    override suspend fun execute(command: ApplicationCommand): ApplicationCommandResult =
        when (command) {
            is ApplicationCommand.AcknowledgeGeneratedIdentity -> acknowledgeGeneratedIdentity(command)
            is ApplicationCommand.CancelGeneratedIdentity -> cancelGeneratedIdentity(command)
            is ApplicationCommand.ImportLocalIdentity -> importLocalIdentity(command)
            is ApplicationCommand.SelectIdentity -> updated { native.selectIdentity(command.identityId.value) }
            is ApplicationCommand.ActivateIdentity -> updated { native.activateIdentity(command.identityId.value) }
            ApplicationCommand.SignOut -> updated { native.signOut() }
            ApplicationCommand.RefreshActiveProfile -> updated { native.refreshActiveProfile() }
            is ApplicationCommand.ConfirmIdentityRemoval -> confirmIdentityRemoval(command)
        }

    override suspend fun prepareLocalIdentity(): GeneratedIdentityRecovery {
        val handle = callNative { native.beginGeneratedIdentity() }
        val requestId = RecoveryRequestId.from(handleIds.next("recovery"))
        return try {
            val identity = handle.account().toIdentitySummary()
            val backup = GeneratedKeyBackup(identity.npub, handle.takeRecoverySecret())
            recoveryMutex.withLock { recoveryHandles[requestId] = handle }
            GeneratedIdentityRecovery(
                requestId = requestId,
                identity = identity,
                expiresAt = UnixSeconds(handle.expiresAtSeconds()),
                backup = backup,
            )
        } catch (error: CancellationException) {
            handle.close()
            throw error
        } catch (error: HarvestCircleRuntimeException) {
            handle.close()
            throw error
        } catch (error: Exception) {
            handle.close()
            throw HarvestCircleRuntimeException(error.toApplicationProblem())
        }
    }

    override suspend fun requestIdentityRemoval(identityId: IdentityId): IdentityRemovalRequest {
        val handle = callNative { native.requestIdentityRemoval(identityId.value) }
        val requestId = RemovalRequestId.from(handleIds.next("removal"))
        return try {
            val requestedIdentity = IdentityId.fromPublicKeyHex(handle.publicKeyHex())
            check(requestedIdentity == identityId) { "Native removal identity does not match the request" }
            removalMutex.withLock { removalHandles[requestId] = handle }
            IdentityRemovalRequest(
                requestId = requestId,
                identityId = requestedIdentity,
                deletesLocalCredential = handle.deletesLocalCredential(),
                signsOut = handle.signsOut(),
                expiresAt = UnixSeconds(handle.expiresAtSeconds()),
            )
        } catch (error: CancellationException) {
            handle.close()
            throw error
        } catch (error: HarvestCircleRuntimeException) {
            handle.close()
            throw error
        } catch (error: Exception) {
            handle.close()
            throw HarvestCircleRuntimeException(error.toApplicationProblem())
        }
    }

    override suspend fun shutdown(): ShutdownReceipt =
        shutdownMutex.withLock {
            shutdownReceipt?.let { return@withLock it }
            closeOutstandingHandles()
            callNative { native.shutdown().toShutdownReceipt() }.also { receipt ->
                if (!receipt.closed) throw incompleteShutdown()
                native.close()
                shutdownReceipt = receipt
            }
        }

    private suspend fun acknowledgeGeneratedIdentity(command: ApplicationCommand.AcknowledgeGeneratedIdentity): ApplicationCommandResult {
        val handle =
            recoveryMutex.withLock { recoveryHandles.remove(command.requestId) }
                ?: throw missingHandle("generated identity recovery", command.context.operationId)
        return try {
            updated(command.context.operationId) {
                native.acknowledgeGeneratedIdentity(command.context.toNative(), handle)
            }
        } finally {
            handle.close()
        }
    }

    private suspend fun cancelGeneratedIdentity(command: ApplicationCommand.CancelGeneratedIdentity): ApplicationCommandResult {
        val handle =
            recoveryMutex.withLock { recoveryHandles.remove(command.requestId) }
                ?: throw missingHandle("generated identity recovery")
        return try {
            callNative { check(native.cancelGeneratedIdentity(handle)) { "Native recovery cancellation was incomplete" } }
            ApplicationCommandResult.Updated(currentSnapshot())
        } finally {
            handle.close()
        }
    }

    private suspend fun importLocalIdentity(command: ApplicationCommand.ImportLocalIdentity): ApplicationCommandResult {
        val bytes = command.secretKey.take().encodeToByteArray()
        return try {
            callNative(command.context.operationId) {
                native.importIdentity(command.context.toNative(), bytes).toApplicationResult()
            }
        } finally {
            bytes.fill(0)
            command.secretKey.clear()
        }
    }

    private suspend fun confirmIdentityRemoval(command: ApplicationCommand.ConfirmIdentityRemoval): ApplicationCommandResult {
        val handle =
            removalMutex.withLock { removalHandles.remove(command.requestId) }
                ?: throw missingHandle("identity removal", command.context.operationId)
        return try {
            updated(command.context.operationId) {
                native.confirmIdentityRemoval(command.context.toNative(), handle)
            }
        } finally {
            handle.close()
        }
    }

    private suspend fun updated(
        operationId: OperationId? = null,
        operation: suspend () -> AppSnapshotDto,
    ): ApplicationCommandResult = callNative(operationId) { ApplicationCommandResult.Updated(operation().toApplicationSnapshot()) }

    private suspend fun closeOutstandingHandles() {
        recoveryMutex.withLock {
            recoveryHandles.values.forEach(NativeGeneratedRecoveryHandle::close)
            recoveryHandles.clear()
        }
        removalMutex.withLock {
            removalHandles.values.forEach(NativeRemovalHandle::close)
            removalHandles.clear()
        }
    }

    private fun missingHandle(
        kind: String,
        operationId: OperationId? = null,
    ): HarvestCircleRuntimeException =
        HarvestCircleRuntimeException(
            ApplicationProblem(
                code = ApplicationErrorCode.InvalidApplicationState,
                category = ApplicationErrorCategory.Lifecycle,
                retryable = false,
                recoveryAction = RecoveryAction.None,
                operationId = operationId,
                safeMessage = "The $kind request is no longer available.",
            ),
        )

    private fun incompleteShutdown(): HarvestCircleRuntimeException =
        HarvestCircleRuntimeException(
            ApplicationProblem(
                code = ApplicationErrorCode.InvalidApplicationState,
                category = ApplicationErrorCategory.Lifecycle,
                retryable = false,
                recoveryAction = RecoveryAction.None,
                operationId = null,
                safeMessage = "The native runtime did not complete shutdown.",
            ),
        )

    private suspend fun <T> callNative(
        fallbackOperationId: OperationId? = null,
        operation: suspend () -> T,
    ): T =
        try {
            operation()
        } catch (error: CancellationException) {
            throw error
        } catch (error: HarvestCircleRuntimeException) {
            throw error
        } catch (error: Exception) {
            throw HarvestCircleRuntimeException(error.toApplicationProblem(fallbackOperationId))
        }

    companion object {
        fun open(developmentMode: Boolean): NativeHarvestCircleRuntime {
            val expectation = verifyNativeCompatibility(compatibilityDescriptor())
            return NativeHarvestCircleRuntime(
                UniFfiNativeCorePort(HarvestCircleAppCore.openCompatible(expectation, developmentMode)),
            )
        }
    }
}

internal fun interface NativeHandleIdSource {
    fun next(kind: String): String
}

private class AtomicNativeHandleIdSource : NativeHandleIdSource {
    private val next = AtomicLong(1)

    override fun next(kind: String): String = "native-$kind:${next.getAndIncrement()}"
}

internal interface NativeCorePort : AutoCloseable {
    fun snapshot(): AppSnapshotDto

    suspend fun bootstrap(): AppSnapshotDto

    suspend fun subscribe(onChange: (SnapshotChangeDto) -> Unit): NativeSubscriptionHandle

    suspend fun beginGeneratedIdentity(): NativeGeneratedRecoveryHandle

    suspend fun acknowledgeGeneratedIdentity(
        context: RequestContextDto,
        request: NativeGeneratedRecoveryHandle,
    ): AppSnapshotDto

    suspend fun cancelGeneratedIdentity(request: NativeGeneratedRecoveryHandle): Boolean

    suspend fun importIdentity(
        context: RequestContextDto,
        secretKey: ByteArray,
    ): AccountCommandReceiptDto

    suspend fun selectIdentity(publicKeyHex: String): AppSnapshotDto

    suspend fun activateIdentity(publicKeyHex: String): AppSnapshotDto

    suspend fun signOut(): AppSnapshotDto

    suspend fun refreshActiveProfile(): AppSnapshotDto

    suspend fun requestIdentityRemoval(publicKeyHex: String): NativeRemovalHandle

    suspend fun confirmIdentityRemoval(
        context: RequestContextDto,
        request: NativeRemovalHandle,
    ): AppSnapshotDto

    suspend fun shutdown(): ShutdownReceiptDto
}

internal interface NativeGeneratedRecoveryHandle : AutoCloseable {
    fun account(): AccountDto

    fun expiresAtSeconds(): Long

    fun takeRecoverySecret(): String
}

internal interface NativeRemovalHandle : AutoCloseable {
    fun publicKeyHex(): String

    fun deletesLocalCredential(): Boolean

    fun signsOut(): Boolean

    fun expiresAtSeconds(): Long
}

internal fun interface NativeSubscriptionHandle : AutoCloseable {
    override fun close()
}

private class UniFfiNativeCorePort(
    private val core: HarvestCircleAppCore,
) : NativeCorePort {
    override fun snapshot(): AppSnapshotDto = core.snapshot()

    override suspend fun bootstrap(): AppSnapshotDto = core.bootstrap()

    override suspend fun subscribe(onChange: (SnapshotChangeDto) -> Unit): NativeSubscriptionHandle {
        val subscription =
            core.subscribeChangesV2(
                object : HarvestCircleChangeObserver {
                    override fun onChange(change: SnapshotChangeDto) = onChange(change)
                },
            )
        return UniFfiNativeSubscriptionHandle(subscription)
    }

    override suspend fun beginGeneratedIdentity(): NativeGeneratedRecoveryHandle =
        UniFfiGeneratedRecoveryHandle(core.beginGeneratedAccountV2())

    override suspend fun acknowledgeGeneratedIdentity(
        context: RequestContextDto,
        request: NativeGeneratedRecoveryHandle,
    ): AppSnapshotDto = core.acknowledgeGeneratedAccountV2(context, request.generated())

    override suspend fun cancelGeneratedIdentity(request: NativeGeneratedRecoveryHandle): Boolean =
        core.cancelGeneratedAccountV2(request.generated())

    override suspend fun importIdentity(
        context: RequestContextDto,
        secretKey: ByteArray,
    ): AccountCommandReceiptDto = core.importAccountV2(context, secretKey)

    override suspend fun selectIdentity(publicKeyHex: String): AppSnapshotDto = core.selectAccount(publicKeyHex)

    override suspend fun activateIdentity(publicKeyHex: String): AppSnapshotDto = core.activateAccount(publicKeyHex)

    override suspend fun signOut(): AppSnapshotDto = core.signOut()

    override suspend fun refreshActiveProfile(): AppSnapshotDto = core.refreshActiveProfile()

    override suspend fun requestIdentityRemoval(publicKeyHex: String): NativeRemovalHandle =
        UniFfiRemovalHandle(core.requestAccountRemoval(publicKeyHex))

    override suspend fun confirmIdentityRemoval(
        context: RequestContextDto,
        request: NativeRemovalHandle,
    ): AppSnapshotDto = core.confirmAccountRemoval(context, request.generated())

    override suspend fun shutdown(): ShutdownReceiptDto = core.shutdownV2()

    override fun close() = core.close()

    private fun NativeGeneratedRecoveryHandle.generated(): GeneratedRecoveryRequest =
        (this as? UniFfiGeneratedRecoveryHandle)?.request
            ?: error("Generated recovery handle does not belong to this native runtime")

    private fun NativeRemovalHandle.generated(): RemovalRequest =
        (this as? UniFfiRemovalHandle)?.request
            ?: error("Removal handle does not belong to this native runtime")
}

private class UniFfiGeneratedRecoveryHandle(
    val request: GeneratedRecoveryRequest,
) : NativeGeneratedRecoveryHandle {
    override fun account(): AccountDto = request.account()

    override fun expiresAtSeconds(): Long = request.expiresAtSeconds()

    override fun takeRecoverySecret(): String = request.takeRecoveryNsec()

    override fun close() = request.close()
}

private class UniFfiRemovalHandle(
    val request: RemovalRequest,
) : NativeRemovalHandle {
    override fun publicKeyHex(): String = request.publicKeyHex()

    override fun deletesLocalCredential(): Boolean = request.deletesLocalCredential()

    override fun signsOut(): Boolean = request.signsOut()

    override fun expiresAtSeconds(): Long = request.expiresAtSeconds()

    override fun close() = request.close()
}

private class UniFfiNativeSubscriptionHandle(
    private val subscription: ObserverSubscription,
) : NativeSubscriptionHandle {
    override fun close() {
        try {
            runBlocking { subscription.unsubscribe() }
        } finally {
            subscription.close()
        }
    }
}
