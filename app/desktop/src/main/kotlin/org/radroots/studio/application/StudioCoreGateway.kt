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

interface RemovalTicket : AutoCloseable

interface GeneratedRecoveryTicket : AutoCloseable {
    val account: AccountDto

    fun takeRecoveryNsec(): String

    suspend fun acknowledge(): AppSnapshotDto

    suspend fun cancel(): Boolean
}

interface StudioCoreGateway : AutoCloseable {
    fun snapshot(): AppSnapshotDto

    suspend fun subscribe(onSnapshot: (AppSnapshotDto) -> Unit): AutoCloseable

    suspend fun bootstrap(): AppSnapshotDto

    suspend fun beginGeneratedAccount(): GeneratedRecoveryTicket

    suspend fun importSecretKey(secretKey: ByteArray): AppSnapshotDto

    suspend fun selectAccount(publicKeyHex: String): AppSnapshotDto

    suspend fun activateAccount(publicKeyHex: String): AppSnapshotDto

    suspend fun signOut(): AppSnapshotDto

    suspend fun refreshActiveProfile(): AppSnapshotDto

    suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket

    suspend fun confirmAccountRemoval(ticket: RemovalTicket): AppSnapshotDto
}

class NativeStudioCoreGateway(
    private val core: StudioAppCore,
) : StudioCoreGateway {
    private val nextRequest = AtomicLong(1)
    override fun snapshot(): AppSnapshotDto = core.snapshot()

    override suspend fun subscribe(onSnapshot: (AppSnapshotDto) -> Unit): AutoCloseable {
        val subscription = core.subscribeChangesV2(
            object : StudioChangeObserver {
                override fun onChange(change: SnapshotChangeDto) {
                    onSnapshot(change.snapshot)
                }
            },
        )
        return NativeSubscription(subscription)
    }

    override suspend fun bootstrap(): AppSnapshotDto = core.bootstrap()

    override suspend fun beginGeneratedAccount(): GeneratedRecoveryTicket =
        NativeGeneratedRecoveryTicket(core, core.beginGeneratedAccountV2())

    override suspend fun importSecretKey(secretKey: ByteArray): AppSnapshotDto =
        try {
            core.importAccountV2(requestContext(), secretKey).snapshot
        } finally {
            secretKey.fill(0)
        }

    override suspend fun selectAccount(publicKeyHex: String): AppSnapshotDto =
        core.selectAccount(publicKeyHex)

    override suspend fun activateAccount(publicKeyHex: String): AppSnapshotDto =
        core.activateAccount(publicKeyHex)

    override suspend fun signOut(): AppSnapshotDto = core.signOut()

    override suspend fun refreshActiveProfile(): AppSnapshotDto = core.refreshActiveProfile()

    override suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket =
        NativeRemovalTicket(core.requestAccountRemoval(publicKeyHex))

    override suspend fun confirmAccountRemoval(ticket: RemovalTicket): AppSnapshotDto {
        require(ticket is NativeRemovalTicket) { "Removal ticket does not belong to native core" }
        return core.confirmAccountRemoval(ticket.request)
    }

    override fun close() {
        runBlocking { core.shutdownV2() }
        core.close()
    }

    private fun requestContext(): RequestContextDto = RequestContextDto(
        requestId = "kotlin:${nextRequest.getAndIncrement()}",
        expectedRevision = core.snapshot().revision,
        deadlineMillis = 30_000UL,
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
