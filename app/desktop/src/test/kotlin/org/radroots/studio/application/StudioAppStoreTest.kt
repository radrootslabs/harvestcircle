package org.radroots.studio.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.radroots.studio.ffi.AccountDto
import org.radroots.studio.ffi.AppLifecycleDto
import org.radroots.studio.ffi.AppSnapshotDto
import org.radroots.studio.ffi.GeneratedAccountDto
import org.radroots.studio.ffi.KeyAvailabilityDto
import org.radroots.studio.ffi.SessionStateDto
import org.radroots.studio.ffi.SignerKindDto
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class StudioAppStoreTest {
    @Test
    fun `bootstraps and ignores stale observer snapshots`() = runTest {
        val gateway = FakeStudioCoreGateway(snapshot(0UL))
        val store = StudioAppStore(gateway, this)

        advanceUntilIdle()
        gateway.emit(snapshot(1UL))
        gateway.emit(snapshot(0UL))

        assertEquals(1UL, store.state.value.snapshot.revision)
        assertFalse(store.state.value.busy)
        store.close()
        assertTrue(gateway.closed)
        assertTrue(gateway.subscriptionClosed)
    }

    @Test
    fun `holds generated secret only until explicit acknowledgement`() = runTest {
        val gateway = FakeStudioCoreGateway(snapshot(0UL))
        val store = StudioAppStore(gateway, this)
        advanceUntilIdle()

        store.generateAccount()
        advanceUntilIdle()

        assertEquals("nsec1secret", store.state.value.generatedKeyBackup?.nsec)
        assertEquals("npub1account", store.state.value.generatedKeyBackup?.npub)
        store.acknowledgeGeneratedKeyBackup()
        assertNull(store.state.value.generatedKeyBackup)
        store.close()
    }

    @Test
    fun `serializes commands while one is active`() = runTest {
        val gateway = FakeStudioCoreGateway(snapshot(0UL))
        val store = StudioAppStore(gateway, this)

        store.signOut()
        advanceUntilIdle()

        assertEquals(0, gateway.signOutCalls)
        store.signOut()
        advanceUntilIdle()
        assertEquals(1, gateway.signOutCalls)
        store.close()
    }
}

private class FakeStudioCoreGateway(
    private var current: AppSnapshotDto,
) : StudioCoreGateway {
    private var observer: ((AppSnapshotDto) -> Unit)? = null
    var closed = false
    var subscriptionClosed = false
    var signOutCalls = 0

    override fun snapshot(): AppSnapshotDto = current

    override fun subscribe(onSnapshot: (AppSnapshotDto) -> Unit): AutoCloseable {
        observer = onSnapshot
        return AutoCloseable { subscriptionClosed = true }
    }

    fun emit(snapshot: AppSnapshotDto) {
        current = snapshot
        observer?.invoke(snapshot)
    }

    override suspend fun bootstrap(): AppSnapshotDto = snapshot(1UL).also(::emit)

    override suspend fun generateAccount(): GeneratedAccountDto {
        val next = snapshot(current.revision + 1UL)
        emit(next)
        return GeneratedAccountDto(account(), next, "nsec1secret")
    }

    override suspend fun importSecretKey(secretKey: String): AppSnapshotDto = current
    override suspend fun selectAccount(publicKeyHex: String): AppSnapshotDto = current
    override suspend fun activateAccount(publicKeyHex: String): AppSnapshotDto = current
    override suspend fun signOut(): AppSnapshotDto = current.also { signOutCalls += 1 }
    override suspend fun refreshActiveProfile(): AppSnapshotDto = current
    override suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket =
        object : RemovalTicket {
            override fun close() = Unit
        }

    override suspend fun confirmAccountRemoval(ticket: RemovalTicket): AppSnapshotDto = current

    override fun close() {
        closed = true
    }
}

private fun snapshot(revision: ULong) = AppSnapshotDto(
    revision = revision,
    lifecycle = AppLifecycleDto.READY,
    lifecycleError = null,
    configuredRelays = emptyList(),
    accounts = emptyList(),
    selectedPublicKeyHex = null,
    session = SessionStateDto.SIGNED_OUT,
    sessionSubjectPublicKeyHex = null,
    sessionError = null,
    activeAccount = null,
    recoverableProblem = null,
)

private fun account() = AccountDto(
    publicKeyHex = "00".repeat(32),
    npub = "npub1account",
    displayLabel = "Account",
    signerKind = SignerKindDto.LOCAL_SECRET,
    keyAvailability = KeyAvailabilityDto.AVAILABLE,
    createdAtSeconds = 0,
    lastUsedAtSeconds = null,
)
