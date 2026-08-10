package org.harvestcircle.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.harvestcircle.ffi.AppLifecycleDto
import org.harvestcircle.ffi.AppSnapshotDto
import org.harvestcircle.ffi.IdentityDto
import org.harvestcircle.ffi.SessionStateDto
import org.harvestcircle.ffi.SignerAvailabilityDto
import org.harvestcircle.ffi.SignerBindingKindDto
import org.harvestcircle.ffi.WireErrorCategory
import org.harvestcircle.ffi.WireErrorCode
import org.harvestcircle.ffi.WireRecoveryAction
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class HarvestCircleAppStoreTest {
    @Test
    fun `bootstraps and ignores stale observer snapshots`() =
        runTest {
            val gateway = FakeHarvestCircleCoreGateway(snapshot(0UL))
            val store = HarvestCircleAppStore(gateway, this)

            advanceUntilIdle()
            gateway.emit(snapshot(1UL))
            gateway.emit(snapshot(0UL))
            advanceUntilIdle()

            assertEquals(1UL, store.state.value.snapshot.revision)
            assertFalse(store.state.value.busy)
            store.close()
            assertTrue(gateway.closed)
            assertTrue(gateway.shutdownCompleted)
            assertTrue(gateway.subscriptionClosed)
            assertEquals(HarvestCircleRoute.CLOSED, store.state.value.route)
        }

    @Test
    fun `holds generated secret only until explicit acknowledgement`() =
        runTest {
            val gateway = FakeHarvestCircleCoreGateway(snapshot(0UL))
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()

            store.generateIdentity()
            advanceUntilIdle()

            assertEquals(
                "nsec1secret",
                store.state.value.generatedKeyBackup
                    ?.revealNsec(),
            )
            assertEquals(
                "npub1identity",
                store.state.value.generatedKeyBackup
                    ?.npub,
            )
            store.acknowledgeGeneratedKeyBackup()
            advanceUntilIdle()
            assertNull(store.state.value.generatedKeyBackup)
            store.close()
        }

    @Test
    fun `cancels staged generated identity without committing it`() =
        runTest {
            val gateway = FakeHarvestCircleCoreGateway(snapshot(0UL))
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()
            val revisionBeforeGeneration = store.state.value.snapshot.revision
            store.generateIdentity()
            advanceUntilIdle()

            store.cancelGeneratedKeyBackup()
            advanceUntilIdle()

            assertNull(store.state.value.generatedKeyBackup)
            assertEquals(revisionBeforeGeneration, store.state.value.snapshot.revision)
            store.close()
        }

    @Test
    fun `partial generated recovery acquisition cancels and closes its native ticket`() =
        runTest {
            val gateway =
                FakeHarvestCircleCoreGateway(snapshot(0UL)).apply {
                    failGeneratedRecoveryRead = true
                }
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()

            store.generateIdentity()
            advanceUntilIdle()

            assertNull(store.state.value.generatedKeyBackup)
            assertEquals(1, gateway.lastGeneratedRecoveryTicket?.cancelCalls)
            assertTrue(gateway.lastGeneratedRecoveryTicket?.closed == true)
            store.close()
        }

    @Test
    fun `failed generated acknowledgement releases one-shot recovery ownership`() =
        runTest {
            val gateway =
                FakeHarvestCircleCoreGateway(snapshot(0UL)).apply {
                    failGeneratedAcknowledgement = true
                }
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()
            store.generateIdentity()
            advanceUntilIdle()

            store.acknowledgeGeneratedKeyBackup()
            advanceUntilIdle()

            assertNull(store.state.value.generatedKeyBackup)
            assertTrue(gateway.lastGeneratedRecoveryTicket?.closed == true)
            assertEquals("fake-generated-request", store.state.value.lastCommandRequestId)
            assertEquals(
                "The generated identity could not be saved. Import the recovery key you saved to try again.",
                store.state.value.problem,
            )
            store.close()
        }

    @Test
    fun `already resolved cancellation clears recovery and reports the state mismatch`() =
        runTest {
            val gateway =
                FakeHarvestCircleCoreGateway(snapshot(0UL)).apply {
                    generatedCancellationResult = false
                }
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()
            store.generateIdentity()
            advanceUntilIdle()

            store.cancelGeneratedKeyBackup()
            advanceUntilIdle()

            assertNull(store.state.value.generatedKeyBackup)
            assertEquals(WireErrorCode.INVALID_APPLICATION_STATE, store.state.value.lastFailureCode)
            assertEquals("fake-generated-request", store.state.value.lastCommandRequestId)
            assertTrue(gateway.lastGeneratedRecoveryTicket?.closed == true)
            store.close()
        }

    @Test
    fun `ignores observer delivery after close`() =
        runTest {
            val gateway = FakeHarvestCircleCoreGateway(snapshot(0UL))
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()
            val revisionAtClose = store.state.value.snapshot.revision

            store.close()
            gateway.emit(snapshot(revisionAtClose + 1UL))
            advanceUntilIdle()

            assertEquals(revisionAtClose, store.state.value.snapshot.revision)
        }

    @Test
    fun `failed removal confirmation clears consumed presentation state`() =
        runTest {
            val gateway =
                FakeHarvestCircleCoreGateway(snapshot(0UL)).apply {
                    failRemovalConfirmation = true
                }
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()

            store.requestIdentityRemoval("00".repeat(32))
            advanceUntilIdle()
            assertEquals("00".repeat(32), store.state.value.pendingRemovalPublicKeyHex)
            store.confirmIdentityRemoval()
            advanceUntilIdle()

            assertNull(store.state.value.pendingRemovalPublicKeyHex)
            assertTrue(gateway.lastRemovalTicket?.closed == true)
            assertEquals(RemovalStatus.FAILED, store.state.value.removalStatus)
            assertEquals("The application command failed.", store.state.value.problem)
            store.close()
        }

    @Test
    fun `serializes commands while one is active`() =
        runTest {
            val gateway = FakeHarvestCircleCoreGateway(snapshot(0UL))
            val store = HarvestCircleAppStore(gateway, this)

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
    fun `projects retryable command rejection without dropping intent`() =
        runTest {
            val gateway = FakeHarvestCircleCoreGateway(snapshot(0UL))
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()
            gateway.nextCommandResult =
                HarvestCircleCommandResult.Rejected(
                    HarvestCircleCommandFailure(
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
            store.retryLastCommand()
            advanceUntilIdle()
            assertEquals(1, gateway.signOutCalls)
            assertEquals(CommandStatus.ACCEPTED, store.state.value.commandStatus)
            store.close()
        }

    @Test
    fun `clears imported secret draft as soon as command is accepted`() =
        runTest {
            val gateway = FakeHarvestCircleCoreGateway(snapshot(0UL))
            val store = HarvestCircleAppStore(gateway, this)
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

    @Test
    fun `bounds imported secret presentation input before transport`() =
        runTest {
            val gateway = FakeHarvestCircleCoreGateway(snapshot(0UL))
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()

            store.editImportDraft("x".repeat(MAX_IMPORT_SECRET_CHARS + 50))

            assertEquals(MAX_IMPORT_SECRET_CHARS, store.state.value.importDraft.length)
            store.close()
        }

    @Test
    fun `projects boot fatal and terminal lifecycle failures`() =
        runTest {
            val booting = snapshot(0UL, AppLifecycleDto.OPENING)
            val bootGateway = FakeHarvestCircleCoreGateway(booting, booting)
            val bootStore = HarvestCircleAppStore(bootGateway, this)
            advanceUntilIdle()
            assertEquals(HarvestCircleRoute.OPENING, bootStore.state.value.route)
            bootStore.close()

            val fatal = snapshot(1UL, AppLifecycleDto.FATAL)
            val gateway = FakeHarvestCircleCoreGateway(fatal, fatal)
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()
            assertEquals(HarvestCircleRoute.FATAL, store.state.value.route)
            store.signOut()
            advanceUntilIdle()
            assertEquals(CommandStatus.FAILED_TERMINAL, store.state.value.commandStatus)
            assertEquals(0, gateway.signOutCalls)

            store.close()
            store.signOut()
            assertEquals(CommandStatus.REJECTED_CLOSED, store.state.value.commandStatus)
        }

    @Test
    fun `disposal waits for native shutdown and fails closed on an incomplete receipt`() =
        runTest {
            val gateway =
                FakeHarvestCircleCoreGateway(snapshot(0UL)).apply {
                    shutdownReceipt = HarvestCircleShutdownReceipt(1UL, closed = false)
                }
            val store = HarvestCircleAppStore(gateway, this)
            advanceUntilIdle()

            store.close()

            assertTrue(gateway.shutdownCompleted)
            assertEquals(HarvestCircleRoute.FATAL, store.state.value.route)
            assertEquals("The application could not shut down safely.", store.state.value.problem)
        }
}

private class FakeHarvestCircleCoreGateway(
    private var current: AppSnapshotDto,
    private val bootstrapSnapshot: AppSnapshotDto = snapshot(1UL),
) : HarvestCircleCoreGateway {
    private var observer: ((AppSnapshotDto) -> Unit)? = null
    var closed = false
    var shutdownCompleted = false
    var shutdownReceipt = HarvestCircleShutdownReceipt(current.revision, closed = true)
    var subscriptionClosed = false
    var signOutCalls = 0
    val importedSecrets = mutableListOf<String>()
    var lastImportBuffer: ByteArray? = null
    var failRemovalConfirmation = false
    var lastRemovalTicket: FakeRemovalTicket? = null
    var nextCommandResult: HarvestCircleCommandResult? = null
    var failGeneratedRecoveryRead = false
    var failGeneratedAcknowledgement = false
    var generatedCancellationResult = true
    var lastGeneratedRecoveryTicket: FakeGeneratedRecoveryTicket? = null

    override fun snapshot(): AppSnapshotDto = current

    override suspend fun subscribeChanges(onChange: (HarvestCircleChange) -> Unit): AutoCloseable {
        observer = { snapshot -> onChange(HarvestCircleChange(snapshot, null)) }
        return AutoCloseable { subscriptionClosed = true }
    }

    override suspend fun execute(command: HarvestCircleCommand): HarvestCircleCommandResult {
        nextCommandResult?.let {
            nextCommandResult = null
            return it
        }
        when (command) {
            is HarvestCircleCommand.ImportIdentity -> {
                lastImportBuffer = command.bytes
                importedSecrets += command.bytes.decodeToString()
                command.bytes.fill(0)
            }
            HarvestCircleCommand.SignOut -> signOutCalls += 1
            else -> Unit
        }
        return HarvestCircleCommandResult.Accepted(
            HarvestCircleCommandReceipt("fake-request", current.revision, current),
        )
    }

    fun emit(snapshot: AppSnapshotDto) {
        current = snapshot
        observer?.invoke(snapshot)
    }

    override suspend fun bootstrap(): AppSnapshotDto = bootstrapSnapshot.also(::emit)

    override suspend fun beginGeneratedIdentity(): GeneratedRecoveryTicket =
        FakeGeneratedRecoveryTicket(
            identity = identity(),
            failRecoveryRead = failGeneratedRecoveryRead,
            failAcknowledgement = failGeneratedAcknowledgement,
            cancellationResult = generatedCancellationResult,
        ) { committed ->
            current = snapshot(current.revision + 1UL)
            emit(current)
            committed(current)
        }.also { lastGeneratedRecoveryTicket = it }

    override suspend fun requestIdentityRemoval(publicKeyHex: String): RemovalTicket = FakeRemovalTicket().also { lastRemovalTicket = it }

    override suspend fun confirmIdentityRemoval(ticket: RemovalTicket): AppSnapshotDto {
        if (failRemovalConfirmation) error("injected confirmation failure")
        return current
    }

    override fun shutdown(): HarvestCircleShutdownReceipt {
        shutdownCompleted = true
        closed = true
        return shutdownReceipt
    }

    override fun close() {
        shutdown()
    }
}

private class FakeGeneratedRecoveryTicket(
    override val identity: IdentityDto,
    private val failRecoveryRead: Boolean,
    private val failAcknowledgement: Boolean,
    private val cancellationResult: Boolean,
    private val commit: (((AppSnapshotDto) -> Unit) -> Unit),
) : GeneratedRecoveryTicket {
    override val requestId: String = "fake-generated-request"
    private var available = true
    var cancelCalls = 0
    var closed = false

    override fun takeRecoveryNsec(): String {
        if (failRecoveryRead) error("injected recovery read failure")
        return "nsec1secret"
    }

    override suspend fun acknowledge(): AppSnapshotDto {
        if (failAcknowledgement) {
            available = false
            throw HarvestCircleGatewayException(
                HarvestCircleCommandFailure(
                    WireErrorCode.KEYRING_UNAVAILABLE,
                    WireErrorCategory.CREDENTIAL,
                    retryable = false,
                    WireRecoveryAction.NONE,
                    requestId,
                    "The generated identity could not be saved. Import the recovery key you saved to try again.",
                ),
            )
        }
        lateinit var snapshot: AppSnapshotDto
        commit { snapshot = it }
        available = false
        return snapshot
    }

    override suspend fun cancel(): Boolean {
        cancelCalls += 1
        return (available && cancellationResult).also { available = false }
    }

    override fun close() {
        closed = true
    }
}

private class FakeRemovalTicket : RemovalTicket {
    override val publicKeyHex: String = "00".repeat(32)
    override val deletesLocalCredential: Boolean = true
    override val signsOut: Boolean = false
    override val expiresAtSeconds: Long = 60
    var closed = false

    override fun close() {
        closed = true
    }
}

private fun snapshot(
    revision: ULong,
    lifecycle: AppLifecycleDto = AppLifecycleDto.READY,
) = AppSnapshotDto(
    revision = revision,
    lifecycle = lifecycle,
    lifecycleError = null,
    configuredRelays = emptyList(),
    identities = emptyList(),
    selectedPublicKeyHex = null,
    session = SessionStateDto.SIGNED_OUT,
    sessionSubjectPublicKeyHex = null,
    sessionError = null,
    activeIdentity = null,
    recoverableProblem = null,
)

private fun identity() =
    IdentityDto(
        publicKeyHex = "00".repeat(32),
        npub = "npub1identity",
        displayLabel = "Identity",
        signerBindingKind = SignerBindingKindDto.LOCAL_KEYRING,
        signerAvailability = SignerAvailabilityDto.AVAILABLE,
        createdAtSeconds = 0,
        lastUsedAtSeconds = null,
    )
