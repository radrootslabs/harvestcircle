package org.harvestcircle.application

import kotlinx.coroutines.async
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.harvestcircle.ffi.ActiveIdentityDto
import org.harvestcircle.ffi.AppLifecycleDto
import org.harvestcircle.ffi.AppSnapshotDto
import org.harvestcircle.ffi.HarvestCircleException
import org.harvestcircle.ffi.IdentityCommandReceiptDto
import org.harvestcircle.ffi.IdentityDto
import org.harvestcircle.ffi.ProfileDto
import org.harvestcircle.ffi.ProfileLoadStateDto
import org.harvestcircle.ffi.RelayConnectionStateDto
import org.harvestcircle.ffi.RequestContextDto
import org.harvestcircle.ffi.SafeErrorDto
import org.harvestcircle.ffi.SessionStateDto
import org.harvestcircle.ffi.ShutdownReceiptDto
import org.harvestcircle.ffi.SignerAvailabilityDto
import org.harvestcircle.ffi.SignerBindingKindDto
import org.harvestcircle.ffi.SnapshotChangeDto
import org.harvestcircle.ffi.WireErrorCategory
import org.harvestcircle.ffi.WireErrorCode
import org.harvestcircle.ffi.WireRecoveryAction
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

class NativeRuntimeMappingsTest {
    @Test
    fun everyGeneratedEnumMapsExhaustively() {
        assertEquals(
            11,
            AppLifecycleDto.entries
                .map(AppLifecycleDto::toApplicationLifecycle)
                .distinct()
                .size,
        )
        assertEquals(
            5,
            SessionStateDto.entries
                .map(SessionStateDto::toSessionLifecycle)
                .distinct()
                .size,
        )
        assertEquals(
            1,
            SignerBindingKindDto.entries
                .map(SignerBindingKindDto::toSignerBindingKind)
                .distinct()
                .size,
        )
        assertEquals(
            3,
            SignerAvailabilityDto.entries
                .map(SignerAvailabilityDto::toSignerAvailability)
                .distinct()
                .size,
        )
        assertEquals(
            5,
            RelayConnectionStateDto.entries
                .map(RelayConnectionStateDto::toRelayConnectionState)
                .distinct()
                .size,
        )
        assertEquals(
            5,
            ProfileLoadStateDto.entries
                .map(ProfileLoadStateDto::toProfileLoadState)
                .distinct()
                .size,
        )
        assertEquals(
            23,
            WireErrorCode.entries
                .map(WireErrorCode::toApplicationErrorCode)
                .distinct()
                .size,
        )
        assertEquals(
            8,
            WireErrorCategory.entries
                .map(WireErrorCategory::toApplicationErrorCategory)
                .distinct()
                .size,
        )
        assertEquals(
            9,
            WireRecoveryAction.entries
                .map(WireRecoveryAction::toRecoveryAction)
                .distinct()
                .size,
        )
    }

    @Test
    fun snapshotMapperPreservesEveryRecordAndOptionalField() {
        val native = populatedSnapshot(revision = 2UL)
        val mapped = native.toApplicationSnapshot()

        assertEquals(SnapshotRevision(2UL), mapped.revision)
        assertEquals(ApplicationLifecycle.Degraded, mapped.lifecycle)
        assertEquals(ApplicationErrorCode.RelayConnectionFailed, mapped.lifecycleProblem?.code)
        assertEquals(native.configuredRelays, mapped.configuredRelays)
        assertEquals(
            native.identities.single().publicKeyHex,
            mapped.identities
                .single()
                .id.value,
        )
        assertEquals(UnixSeconds(2), mapped.identities.single().lastUsedAt)
        assertEquals(mapped.identities.single().id, mapped.selectedIdentityId)
        assertEquals(mapped.identities.single().id, mapped.sessionSubjectIdentityId)
        assertEquals(ProfileLoadState.Fresh, mapped.activeIdentity?.profileState)
        assertEquals("display", mapped.activeIdentity?.profile?.displayName)
        assertEquals(ApplicationErrorCode.CredentialMissing, mapped.sessionProblem?.code)
        assertEquals(ApplicationErrorCode.StorageCorrupt, mapped.recoverableProblem?.code)

        val empty = emptySnapshot().toApplicationSnapshot()
        assertNull(empty.lifecycleProblem)
        assertNull(empty.selectedIdentityId)
        assertNull(empty.sessionSubjectIdentityId)
        assertNull(empty.sessionProblem)
        assertNull(empty.activeIdentity)
        assertNull(empty.recoverableProblem)
    }

    @Test
    fun receiptChangeContextAndShutdownMappingsPreserveRevisions() {
        val snapshot = populatedSnapshot(revision = 2UL)
        val result =
            IdentityCommandReceiptDto("operation-7", 2UL, snapshot)
                .toApplicationResult()
        assertEquals(OperationId.from("operation-7"), result.operationId)
        assertEquals(SnapshotRevision(2UL), result.committedRevision)

        val change = SnapshotChangeDto(snapshot, 1UL).toApplicationChange()
        assertEquals(SnapshotRevision(1UL), change.previousRevision)
        assertEquals(SnapshotRevision(2UL), change.snapshot.revision)

        val context = RequestContext(OperationId.from("operation-7"), SnapshotRevision(1UL), 5_000UL).toNative()
        assertEquals("operation-7", context.requestId)
        assertEquals(1UL, context.expectedRevision)
        assertEquals(5_000UL, context.deadlineMillis)

        assertEquals(
            ShutdownReceipt(SnapshotRevision(2UL), true),
            ShutdownReceiptDto(2UL, true).toShutdownReceipt(),
        )
    }

    @Test
    fun nativeAndUnknownErrorsBecomeStructuredAndSecretSafe() {
        val native =
            HarvestCircleException
                .Failure(
                    code = WireErrorCode.CREDENTIAL_MISSING,
                    category = WireErrorCategory.CREDENTIAL,
                    retryable = false,
                    recoveryAction = WireRecoveryAction.REPAIR_CREDENTIAL,
                    correlationId = "operation-7",
                    safeMessage = "The local credential is unavailable.",
                ).toApplicationProblem()
        assertEquals(ApplicationErrorCode.CredentialMissing, native.code)
        assertEquals(OperationId.from("operation-7"), native.operationId)
        assertEquals(RecoveryAction.RepairCredential, native.recoveryAction)

        val unknown = IllegalStateException("sensitive detail").toApplicationProblem(OperationId.from("fallback-1"))
        assertEquals(ApplicationErrorCode.Internal, unknown.code)
        assertEquals("The application command failed.", unknown.safeMessage)
        assertEquals(OperationId.from("fallback-1"), unknown.operationId)
        assertFalse(unknown.toString().contains("sensitive detail"))
    }

    @Test
    fun onlyTheImplementedLocalKeyringBindingIsPublished() {
        assertEquals(SignerBindingKind.LocalKeyring, SignerBindingKindDto.LOCAL_KEYRING.toSignerBindingKind())
        assertEquals(1, SignerBindingKindDto.entries.size)
    }
}

class NativeHarvestCircleRuntimeTest {
    @Test
    fun adapterOwnsCommandsHandlesAndIdempotentShutdown() =
        runTest {
            val port = FakeNativeCorePort()
            val runtime = NativeHarvestCircleRuntime(port, NativeHandleIdSource { kind -> "$kind-1" })
            val context = RequestContext(OperationId.from("operation-7"), SnapshotRevision(2UL), 5_000UL)

            val secret = SecretKeyInput.from("nsec1boundedsecret")
            val imported =
                runtime.execute(ApplicationCommand.ImportLocalIdentity(secret, context))
            assertIs<ApplicationCommandResult.Committed>(imported)
            assertEquals("nsec1boundedsecret", port.importedSecret?.decodeToString())
            assertFailsWith<IllegalStateException> { secret.take() }

            val recovery = runtime.prepareLocalIdentity()
            assertEquals("recovery-1", recovery.requestId.value)
            assertEquals("nsec1generated", recovery.backup.revealNsec())
            runtime.execute(ApplicationCommand.AcknowledgeGeneratedIdentity(recovery.requestId, context))
            assertTrue(port.generated.closed)

            val identityId = IdentityId.fromPublicKeyHex(nativeIdentity().publicKeyHex)
            val removal = runtime.requestIdentityRemoval(identityId)
            assertEquals("removal-1", removal.requestId.value)
            runtime.execute(ApplicationCommand.ConfirmIdentityRemoval(removal.requestId, context))
            assertTrue(port.removal.closed)

            val first = runtime.shutdown()
            val repeated = runtime.shutdown()
            assertEquals(first, repeated)
            assertEquals(1, port.shutdownCalls)
            assertTrue(port.closed)
        }

    @Test
    fun observerChangesPreservePredecessorRevisionAndCloseSubscription() =
        runTest {
            val port = FakeNativeCorePort()
            val runtime = NativeHarvestCircleRuntime(port)
            val pending = async { runtime.changes().first() }
            runCurrent()

            port.emit(SnapshotChangeDto(populatedSnapshot(2UL), 1UL))
            val change = pending.await()

            assertEquals(SnapshotRevision(1UL), change.previousRevision)
            assertEquals(SnapshotRevision(2UL), change.snapshot.revision)
            runCurrent()
            assertTrue(port.subscriptionClosed)
        }
}

private class FakeNativeCorePort : NativeCorePort {
    val generated = FakeGeneratedRecoveryHandle()
    val removal = FakeRemovalHandle()
    var importedSecret: ByteArray? = null
    var shutdownCalls = 0
    var closed = false
    var subscriptionClosed = false
    private var observer: ((SnapshotChangeDto) -> Unit)? = null
    private val snapshot = populatedSnapshot(2UL)

    override fun snapshot(): AppSnapshotDto = snapshot

    override suspend fun bootstrap(): AppSnapshotDto = snapshot

    override suspend fun subscribe(onChange: (SnapshotChangeDto) -> Unit): NativeSubscriptionHandle {
        observer = onChange
        return NativeSubscriptionHandle { subscriptionClosed = true }
    }

    fun emit(change: SnapshotChangeDto) {
        checkNotNull(observer)(change)
    }

    override suspend fun beginGeneratedIdentity(): NativeGeneratedRecoveryHandle = generated

    override suspend fun acknowledgeGeneratedIdentity(
        context: RequestContextDto,
        request: NativeGeneratedRecoveryHandle,
    ): AppSnapshotDto {
        assertEquals("operation-7", context.requestId)
        assertEquals(generated, request)
        return snapshot
    }

    override suspend fun cancelGeneratedIdentity(request: NativeGeneratedRecoveryHandle): Boolean = true

    override suspend fun importIdentity(
        context: RequestContextDto,
        secretKey: ByteArray,
    ): IdentityCommandReceiptDto {
        importedSecret = secretKey.copyOf()
        return IdentityCommandReceiptDto(context.requestId, snapshot.revision, snapshot)
    }

    override suspend fun selectIdentity(publicKeyHex: String): AppSnapshotDto = snapshot

    override suspend fun activateIdentity(publicKeyHex: String): AppSnapshotDto = snapshot

    override suspend fun signOut(): AppSnapshotDto = emptySnapshot()

    override suspend fun refreshActiveProfile(): AppSnapshotDto = snapshot

    override suspend fun requestIdentityRemoval(publicKeyHex: String): NativeRemovalHandle = removal

    override suspend fun confirmIdentityRemoval(
        context: RequestContextDto,
        request: NativeRemovalHandle,
    ): AppSnapshotDto {
        assertEquals("operation-7", context.requestId)
        assertEquals(removal, request)
        return snapshot
    }

    override suspend fun shutdown(): ShutdownReceiptDto {
        shutdownCalls += 1
        return ShutdownReceiptDto(snapshot.revision, true)
    }

    override fun close() {
        closed = true
    }
}

private class FakeGeneratedRecoveryHandle : NativeGeneratedRecoveryHandle {
    var closed = false

    override fun identity(): IdentityDto = nativeIdentity()

    override fun expiresAtSeconds(): Long = 100

    override fun takeRecoverySecret(): String = "nsec1generated"

    override fun close() {
        closed = true
    }
}

private class FakeRemovalHandle : NativeRemovalHandle {
    var closed = false

    override fun publicKeyHex(): String = nativeIdentity().publicKeyHex

    override fun deletesLocalCredential(): Boolean = true

    override fun signsOut(): Boolean = true

    override fun expiresAtSeconds(): Long = 100

    override fun close() {
        closed = true
    }
}

private fun populatedSnapshot(revision: ULong): AppSnapshotDto {
    val identity = nativeIdentity()
    return AppSnapshotDto(
        revision = revision,
        lifecycle = AppLifecycleDto.DEGRADED,
        lifecycleError = safeError(WireErrorCode.RELAY_CONNECTION_FAILED),
        configuredRelays = listOf("wss://relay.example"),
        identities = listOf(identity),
        selectedPublicKeyHex = identity.publicKeyHex,
        session = SessionStateDto.ACTIVE,
        sessionSubjectPublicKeyHex = identity.publicKeyHex,
        sessionError = safeError(WireErrorCode.CREDENTIAL_MISSING),
        activeIdentity =
            ActiveIdentityDto(
                identity = identity,
                relayState = RelayConnectionStateDto.CONNECTED,
                profileState = ProfileLoadStateDto.FRESH,
                profile = ProfileDto("name", "display", "name@example.com", "about", "https://example.com/p.png"),
            ),
        recoverableProblem = safeError(WireErrorCode.STORAGE_CORRUPT),
    )
}

private fun emptySnapshot(): AppSnapshotDto =
    AppSnapshotDto(
        revision = 0UL,
        lifecycle = AppLifecycleDto.READY,
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

private fun nativeIdentity(): IdentityDto =
    IdentityDto(
        publicKeyHex = "01".repeat(32),
        npub = "npub1identity",
        displayLabel = "Identity",
        signerBindingKind = SignerBindingKindDto.LOCAL_KEYRING,
        signerAvailability = SignerAvailabilityDto.AVAILABLE,
        createdAtSeconds = 1,
        lastUsedAtSeconds = 2,
    )

private fun safeError(code: WireErrorCode): SafeErrorDto =
    SafeErrorDto(
        code = code,
        category = WireErrorCategory.STORAGE,
        retryable = false,
        recoveryAction = WireRecoveryAction.RETRY,
        message = "A safe problem occurred.",
    )
