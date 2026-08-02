package org.radroots.studio.application

import org.radroots.studio.ffi.AppSnapshotDto
import org.radroots.studio.ffi.GeneratedAccountDto
import org.radroots.studio.ffi.ObserverSubscription
import org.radroots.studio.ffi.RemovalRequest
import org.radroots.studio.ffi.StudioAppCore
import org.radroots.studio.ffi.StudioObserver

interface RemovalTicket : AutoCloseable

interface StudioCoreGateway : AutoCloseable {
    fun snapshot(): AppSnapshotDto

    fun subscribe(onSnapshot: (AppSnapshotDto) -> Unit): AutoCloseable

    suspend fun bootstrap(): AppSnapshotDto

    suspend fun generateAccount(): GeneratedAccountDto

    suspend fun importSecretKey(secretKey: String): AppSnapshotDto

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
    override fun snapshot(): AppSnapshotDto = core.snapshot()

    override fun subscribe(onSnapshot: (AppSnapshotDto) -> Unit): AutoCloseable {
        val subscription = core.subscribe(
            object : StudioObserver {
                override fun onSnapshotChanged(snapshot: AppSnapshotDto) {
                    onSnapshot(snapshot)
                }
            },
        )
        return NativeSubscription(subscription)
    }

    override suspend fun bootstrap(): AppSnapshotDto = core.bootstrap()

    override suspend fun generateAccount(): GeneratedAccountDto = core.generateAccount()

    override suspend fun importSecretKey(secretKey: String): AppSnapshotDto =
        core.importSecretKey(secretKey)

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
        core.shutdown()
        core.close()
    }
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
