package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class HarvestCirclePresenterTest {
    @Test
    fun bootstrapAndObserverChangesPreserveMonotonicSnapshots() =
        runTest {
            val runtime = FakePresenterRuntime()
            val presenter = presenter(runtime)
            runCurrent()

            assertEquals(1UL, presenter.state.value.snapshot.revision.value)
            runtime.emit(snapshot(3UL))
            runtime.emitChange(snapshot(2UL), SnapshotRevision(1UL))
            runCurrent()

            assertEquals(3UL, presenter.state.value.snapshot.revision.value)
            assertFalse(presenter.state.value.busy)
            presenter.close()
        }

    @Test
    fun revisionGapResnapshotsAndAcceptsOnlyTheAuthoritativeLatestState() =
        runTest {
            val runtime = FakePresenterRuntime()
            val presenter = presenter(runtime)
            runCurrent()
            val callsBeforeGap = runtime.currentSnapshotCalls
            runtime.setCurrent(snapshot(5UL))

            runtime.emitChange(snapshot(4UL), SnapshotRevision(2UL))
            runCurrent()

            assertEquals(5UL, presenter.state.value.snapshot.revision.value)
            assertEquals(callsBeforeGap + 1, runtime.currentSnapshotCalls)
            presenter.close()
        }

    @Test
    fun duplicateAndStaleChangesAreIgnoredWithoutResnapshotting() =
        runTest {
            val runtime = FakePresenterRuntime()
            val presenter = presenter(runtime)
            runCurrent()
            runtime.emit(snapshot(2UL))
            runCurrent()
            val callsBeforeStale = runtime.currentSnapshotCalls

            runtime.emitChange(snapshot(2UL), SnapshotRevision(1UL))
            runtime.emitChange(snapshot(1UL), null)
            runCurrent()

            assertEquals(2UL, presenter.state.value.snapshot.revision.value)
            assertEquals(callsBeforeStale, runtime.currentSnapshotCalls)
            presenter.close()
        }

    @Test
    fun insufficientGapResnapshotSurfacesATypedTerminalProblem() =
        runTest {
            val runtime = FakePresenterRuntime()
            val presenter = presenter(runtime)
            runCurrent()
            runtime.setCurrent(snapshot(3UL))

            runtime.emitChange(snapshot(5UL), SnapshotRevision(3UL))
            runCurrent()

            assertEquals(1UL, presenter.state.value.snapshot.revision.value)
            assertEquals(
                ApplicationErrorCode.ObserverRegistrationFailed,
                presenter.state.value.lastProblem
                    ?.code,
            )
            assertEquals(CommandStatus.FAILED_TERMINAL, presenter.state.value.commandStatus)
            presenter.close()
        }

    @Test
    fun busyAdmissionRejectsOverlappingCommands() =
        runTest {
            val bootstrapGate = CompletableDeferred<Unit>()
            val runtime = FakePresenterRuntime(bootstrapGate = bootstrapGate)
            val presenter = presenter(runtime)
            runCurrent()

            presenter.dispatch(HarvestCircleIntent.SignOut)

            assertEquals(CommandStatus.REJECTED_BUSY, presenter.state.value.commandStatus)
            assertEquals(0, runtime.executeCalls)
            bootstrapGate.complete(Unit)
            advanceUntilIdle()
            presenter.close()
        }

    @Test
    fun retryReusesTheOriginalInjectedOperationIdentity() =
        runTest {
            val runtime = FakePresenterRuntime()
            val ids = DeterministicOperationIds()
            val presenter = presenter(runtime, ids)
            runCurrent()
            runtime.nextFailure = problem(retryable = true, operationId = OperationId.from(TEST_OPERATION_ID))

            presenter.dispatch(HarvestCircleIntent.SignOut)
            advanceUntilIdle()

            assertEquals(CommandStatus.FAILED_RETRYABLE, presenter.state.value.commandStatus)
            assertEquals(OperationId.from(TEST_OPERATION_ID), presenter.state.value.lastCommandOperationId)
            presenter.dispatch(HarvestCircleIntent.RetryLastCommand)
            advanceUntilIdle()

            assertEquals(CommandStatus.ACCEPTED, presenter.state.value.commandStatus)
            assertEquals(2, runtime.executeCalls)
            assertEquals(1, ids.calls)
            presenter.close()
        }

    @Test
    fun terminalFailureCannotBeRetried() =
        runTest {
            val runtime = FakePresenterRuntime()
            val presenter = presenter(runtime)
            runCurrent()
            runtime.nextFailure = problem(retryable = false)

            presenter.dispatch(HarvestCircleIntent.RefreshActiveProfile)
            advanceUntilIdle()
            presenter.dispatch(HarvestCircleIntent.RetryLastCommand)

            assertEquals(CommandStatus.FAILED_TERMINAL, presenter.state.value.commandStatus)
            assertEquals("This action cannot be retried safely.", presenter.state.value.problem)
            assertEquals(1, runtime.executeCalls)
            presenter.close()
        }

    @Test
    fun closeCancelsAnInFlightCommandBeforeNativeShutdown() =
        runTest {
            val executeGate = CompletableDeferred<Unit>()
            val runtime = FakePresenterRuntime(executeGate = executeGate)
            val presenter = presenter(runtime)
            runCurrent()
            presenter.dispatch(HarvestCircleIntent.SignOut)
            runCurrent()
            assertTrue(presenter.state.value.busy)

            val receipt = async { presenter.close() }
            advanceUntilIdle()

            assertTrue(runtime.executeCancelled)
            assertTrue(runtime.shutdownCalled)
            assertEquals(HarvestCircleRoute.CLOSED, presenter.state.value.route)
            val first = receipt.await()
            assertTrue(first?.closed == true)
            assertEquals(first, presenter.close())
            assertEquals(1, runtime.shutdownCalls)
        }

    @Test
    fun cancelledCloseCanBeRetriedByDisposalFallback() =
        runTest {
            val shutdownGate = CompletableDeferred<Unit>()
            val runtime = FakePresenterRuntime(shutdownGate = shutdownGate)
            val presenter = presenter(runtime)
            runCurrent()

            val interrupted = async { presenter.close() }
            runCurrent()
            interrupted.cancel()
            runCurrent()
            shutdownGate.complete(Unit)

            val receipt = presenter.close()

            assertTrue(receipt?.closed == true)
            assertEquals(2, runtime.shutdownCalls)
        }

    @Test
    fun generatedRecoveryIsOneUseAndClearedOnCancellation() =
        runTest {
            val runtime = FakePresenterRuntime()
            val presenter = presenter(runtime)
            runCurrent()

            presenter.dispatch(HarvestCircleIntent.GenerateIdentity)
            advanceUntilIdle()
            val backup = presenter.state.value.generatedKeyBackup ?: error("recovery backup")
            assertEquals("nsec1presenter-secret", backup.revealNsec())

            presenter.dispatch(HarvestCircleIntent.CancelGeneratedRecovery)
            advanceUntilIdle()

            assertNull(presenter.state.value.generatedKeyBackup)
            assertEquals(1, runtime.generatedCancellationCalls)
            assertFailsWith<IllegalStateException> { backup.revealNsec() }
            presenter.close()
        }

    @Test
    fun importDraftIsBoundedClearedAndConsumedOnce() =
        runTest {
            val runtime = FakePresenterRuntime()
            val presenter = presenter(runtime)
            runCurrent()

            presenter.dispatch(HarvestCircleIntent.EditImportDraft("nsec1" + "x".repeat(200)))
            assertEquals(MAX_IMPORT_SECRET_CHARS, presenter.state.value.importDraft.length)
            presenter.dispatch(HarvestCircleIntent.ImportIdentity)
            assertEquals("", presenter.state.value.importDraft)
            advanceUntilIdle()

            assertEquals(1, runtime.importedSecrets.size)
            assertEquals(MAX_IMPORT_SECRET_CHARS, runtime.importedSecrets.single().length)
            presenter.close()
        }

    @Test
    fun removalCancellationReleasesTheRuntimeRequest() =
        runTest {
            val runtime = FakePresenterRuntime()
            val presenter = presenter(runtime)
            runCurrent()
            val identityId = IdentityId.fromPublicKeyHex("01".repeat(32))

            presenter.dispatch(HarvestCircleIntent.RequestIdentityRemoval(identityId))
            advanceUntilIdle()
            assertEquals(identityId, presenter.state.value.pendingRemovalIdentityId)
            presenter.dispatch(HarvestCircleIntent.CancelIdentityRemoval)
            advanceUntilIdle()

            assertNull(presenter.state.value.pendingRemovalIdentityId)
            assertEquals(1, runtime.removalCancellationCalls)
            presenter.close()
        }

    private fun TestScope.presenter(
        runtime: FakePresenterRuntime,
        ids: DeterministicOperationIds = DeterministicOperationIds(),
    ): HarvestCirclePresenter =
        HarvestCirclePresenter(
            runtime = runtime,
            scope = this,
            clock = ApplicationClock { UnixSeconds(10) },
            operationIds = ids,
        )
}

private class DeterministicOperationIds : OperationIdSource {
    var calls = 0

    override fun next(): OperationId {
        calls += 1
        return OperationId.from("01890f3e-7b1c-7000-8000-${calls.toString().padStart(12, '0')}")
    }
}

private class FakePresenterRuntime(
    private val bootstrapGate: CompletableDeferred<Unit>? = null,
    private val executeGate: CompletableDeferred<Unit>? = null,
    private val shutdownGate: CompletableDeferred<Unit>? = null,
) : HarvestCircleRuntime {
    override val buildInfo: BuildInfo = BuildInfo.unknown()

    private val changes = MutableSharedFlow<ApplicationChange>(extraBufferCapacity = 8)
    private var current = snapshot(0UL)
    var currentSnapshotCalls = 0
    var nextFailure: ApplicationProblem? = null
    var executeCalls = 0
    var executeCancelled = false
    var shutdownCalled = false
    var shutdownCalls = 0
    var generatedCancellationCalls = 0
    var removalCancellationCalls = 0
    val importedSecrets = mutableListOf<String>()

    override suspend fun bootstrap(): ApplicationSnapshot {
        bootstrapGate?.await()
        return snapshot(1UL).also { current = it }
    }

    override fun currentSnapshot(): ApplicationSnapshot {
        currentSnapshotCalls += 1
        return current
    }

    override fun changes(): Flow<ApplicationChange> = changes

    override suspend fun execute(command: ApplicationCommand): ApplicationCommandResult {
        executeCalls += 1
        nextFailure?.let {
            nextFailure = null
            throw ApplicationFailure(it)
        }
        try {
            executeGate?.await()
        } catch (error: CancellationException) {
            executeCancelled = true
            throw error
        }
        when (command) {
            is ApplicationCommand.ImportLocalIdentity -> importedSecrets += command.secretKey.take()
            is ApplicationCommand.CancelGeneratedIdentity -> generatedCancellationCalls += 1
            else -> Unit
        }
        return ApplicationCommandResult.Updated(current)
    }

    override suspend fun prepareLocalIdentity(): GeneratedIdentityRecovery =
        GeneratedIdentityRecovery(
            requestId = RecoveryRequestId.from("recovery-1"),
            identity = identity(),
            expiresAt = UnixSeconds(60),
            backup = GeneratedKeyBackup("npub1presenter", "nsec1presenter-secret"),
        )

    override suspend fun requestIdentityRemoval(identityId: IdentityId): IdentityRemovalRequest =
        IdentityRemovalRequest(
            requestId = RemovalRequestId.from("removal-1"),
            identityId = identityId,
            deletesLocalCredential = true,
            signsOut = false,
            expiresAt = UnixSeconds(60),
        )

    override suspend fun cancelIdentityRemoval(requestId: RemovalRequestId): Boolean {
        removalCancellationCalls += 1
        return true
    }

    override suspend fun shutdown(): ShutdownReceipt {
        shutdownCalled = true
        shutdownCalls += 1
        shutdownGate?.await()
        return ShutdownReceipt(current.revision, closed = true)
    }

    fun emit(snapshot: ApplicationSnapshot) {
        val previousRevision = current.revision
        current = snapshot
        emitChange(snapshot, previousRevision)
    }

    fun emitChange(
        snapshot: ApplicationSnapshot,
        previousRevision: SnapshotRevision?,
    ) {
        check(changes.tryEmit(ApplicationChange(snapshot, previousRevision)))
    }

    fun setCurrent(snapshot: ApplicationSnapshot) {
        current = snapshot
    }
}

private fun problem(
    retryable: Boolean,
    operationId: OperationId? = null,
) = ApplicationProblem(
    code = ApplicationErrorCode.StorageUnavailable,
    category = ApplicationErrorCategory.Storage,
    retryable = retryable,
    recoveryAction = if (retryable) RecoveryAction.Retry else RecoveryAction.None,
    operationId = operationId,
    safeMessage = "Storage is temporarily unavailable.",
)

private fun identity() =
    IdentitySummary(
        id = IdentityId.fromPublicKeyHex("01".repeat(32)),
        npub = "npub1presenter",
        displayLabel = "Identity",
        signer = SignerBindingSummary(SignerBindingKind.LocalKeyring, SignerAvailability.Available),
        createdAt = UnixSeconds(1),
        lastUsedAt = null,
    )

private fun snapshot(revision: ULong) =
    ApplicationSnapshot(
        revision = SnapshotRevision(revision),
        lifecycle = ApplicationLifecycle.Ready,
        lifecycleProblem = null,
        configuredRelays = emptyList(),
        identities = emptyList(),
        selectedIdentityId = null,
        session = SessionLifecycle.SignedOut,
        sessionSubjectIdentityId = null,
        sessionProblem = null,
        activeIdentity = null,
        recoverableProblem = null,
    )

private const val TEST_OPERATION_ID = "01890f3e-7b1c-7000-8000-000000000001"
