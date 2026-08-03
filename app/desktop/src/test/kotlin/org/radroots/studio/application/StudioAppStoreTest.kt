package org.radroots.studio.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.radroots.studio.ffi.AccountDto
import org.radroots.studio.ffi.AppLifecycleDto
import org.radroots.studio.ffi.AppSnapshotDto
import org.radroots.studio.ffi.KeyAvailabilityDto
import org.radroots.studio.ffi.SessionStateDto
import org.radroots.studio.ffi.SignerKindDto
import org.radroots.studio.ffi.WireErrorCategory
import org.radroots.studio.ffi.WireErrorCode
import org.radroots.studio.ffi.WireRecoveryAction
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
        advanceUntilIdle()

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

        assertEquals("nsec1secret", store.state.value.generatedKeyBackup?.revealNsec())
        assertEquals("npub1account", store.state.value.generatedKeyBackup?.npub)
        store.acknowledgeGeneratedKeyBackup()
        advanceUntilIdle()
        assertNull(store.state.value.generatedKeyBackup)
        store.close()
    }

    @Test
    fun `ignores observer delivery after close`() = runTest {
        val gateway = FakeStudioCoreGateway(snapshot(0UL))
        val store = StudioAppStore(gateway, this)
        advanceUntilIdle()
        val revisionAtClose = store.state.value.snapshot.revision

        store.close()
        gateway.emit(snapshot(revisionAtClose + 1UL))
        advanceUntilIdle()

        assertEquals(revisionAtClose, store.state.value.snapshot.revision)
    }

    @Test
    fun `failed removal confirmation clears consumed presentation state`() = runTest {
        val gateway = FakeStudioCoreGateway(snapshot(0UL)).apply {
            failRemovalConfirmation = true
        }
        val store = StudioAppStore(gateway, this)
        advanceUntilIdle()

        store.requestAccountRemoval("00".repeat(32))
        advanceUntilIdle()
        assertEquals("00".repeat(32), store.state.value.pendingRemovalPublicKeyHex)
        store.confirmAccountRemoval()
        advanceUntilIdle()

        assertNull(store.state.value.pendingRemovalPublicKeyHex)
        assertTrue(gateway.lastRemovalTicket?.closed == true)
        assertEquals("The application command failed.", store.state.value.problem)
        store.close()
    }

    @Test
    fun `serializes commands while one is active`() = runTest {
        val gateway = FakeStudioCoreGateway(snapshot(0UL))
        val store = StudioAppStore(gateway, this)

        store.signOut()
        assertEquals(CommandStatus.REJECTED_BUSY, store.state.value.commandStatus)
        advanceUntilIdle()

        assertEquals(0, gateway.signOutCalls)
        store.signOut()
        advanceUntilIdle()
        assertEquals(1, gateway.signOutCalls)
        store.close()
    }

    @Test
    fun `projects retryable command rejection without dropping intent`() = runTest {
        val gateway = FakeStudioCoreGateway(snapshot(0UL))
        val store = StudioAppStore(gateway, this)
        advanceUntilIdle()
        gateway.nextCommandResult = StudioCommandResult.Rejected(
            StudioCommandFailure(
                WireErrorCode.STORAGE_UNAVAILABLE,
                WireErrorCategory.STORAGE,
                retryable = true,
                WireRecoveryAction.RETRY,
                "request-retry",
                "Storage is temporarily unavailable.",
            ),
        )

        store.signOut()
        advanceUntilIdle()

        assertEquals(CommandStatus.FAILED_RETRYABLE, store.state.value.commandStatus)
        assertEquals("request-retry", store.state.value.lastCommandRequestId)
        assertEquals("Storage is temporarily unavailable.", store.state.value.problem)
        store.close()
    }

    @Test
    fun `clears imported secret draft as soon as command is accepted`() = runTest {
        val gateway = FakeStudioCoreGateway(snapshot(0UL))
        val store = StudioAppStore(gateway, this)
        advanceUntilIdle()
        store.editImportDraft("nsec1secret")

        store.importSecretKey()

        assertEquals("", store.state.value.importDraft)
        assertEquals(emptyList(), gateway.importedSecrets)
        advanceUntilIdle()
        assertEquals(listOf("nsec1secret"), gateway.importedSecrets)
        assertEquals(true, gateway.lastImportBuffer?.all { it == 0.toByte() })
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
    val importedSecrets = mutableListOf<String>()
    var lastImportBuffer: ByteArray? = null
    var failRemovalConfirmation = false
    var lastRemovalTicket: FakeRemovalTicket? = null
    var nextCommandResult: StudioCommandResult? = null

    override fun snapshot(): AppSnapshotDto = current

    override suspend fun subscribeChanges(onChange: (StudioChange) -> Unit): AutoCloseable {
        observer = { snapshot -> onChange(StudioChange(snapshot, null)) }
        return AutoCloseable { subscriptionClosed = true }
    }

    override suspend fun execute(command: StudioCommand): StudioCommandResult {
        nextCommandResult?.let {
            nextCommandResult = null
            return it
        }
        when (command) {
            is StudioCommand.ImportAccount -> {
                lastImportBuffer = command.bytes
                importedSecrets += command.bytes.decodeToString()
                command.bytes.fill(0)
            }
            StudioCommand.SignOut -> signOutCalls += 1
            else -> Unit
        }
        return StudioCommandResult.Accepted(
            StudioCommandReceipt("fake-request", current.revision, current),
        )
    }

    fun emit(snapshot: AppSnapshotDto) {
        current = snapshot
        observer?.invoke(snapshot)
    }

    override suspend fun bootstrap(): AppSnapshotDto = snapshot(1UL).also(::emit)

    override suspend fun beginGeneratedAccount(): GeneratedRecoveryTicket =
        FakeGeneratedRecoveryTicket(account()) { committed ->
            current = snapshot(current.revision + 1UL)
            emit(current)
            committed(current)
        }

    override suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket =
        FakeRemovalTicket().also { lastRemovalTicket = it }

    override suspend fun confirmAccountRemoval(ticket: RemovalTicket): AppSnapshotDto {
        if (failRemovalConfirmation) error("injected confirmation failure")
        return current
    }

    override fun close() {
        closed = true
    }
}

private class FakeGeneratedRecoveryTicket(
    override val account: AccountDto,
    private val commit: (((AppSnapshotDto) -> Unit) -> Unit),
) : GeneratedRecoveryTicket {
    private var available = true

    override fun takeRecoveryNsec(): String = "nsec1secret"

    override suspend fun acknowledge(): AppSnapshotDto {
        lateinit var snapshot: AppSnapshotDto
        commit { snapshot = it }
        available = false
        return snapshot
    }

    override suspend fun cancel(): Boolean = available.also { available = false }

    override fun close() = Unit
}

private class FakeRemovalTicket : RemovalTicket {
    var closed = false

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
