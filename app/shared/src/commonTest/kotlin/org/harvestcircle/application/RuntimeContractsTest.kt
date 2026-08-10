package org.harvestcircle.application

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RuntimeContractsTest {
    @Test
    fun buildInfoRejectsUnknownAndDirtyReleaseInputsAndKeepsDiagnosticsSafe() {
        val clean =
            BuildInfo(
                sourceCommit = "a".repeat(40),
                sourceDirty = BuildDirtyState.Clean,
                radrootsRevision = "b".repeat(40),
                rustToolchain = "1.97.1",
                javaToolchain = "21.0.11",
                kotlinToolchain = "2.4.10",
                provenanceDigest = "d".repeat(64),
                sourceDateEpoch = 1_700_000_000UL,
                ffiContractId = "harvestcircle-desktop-ffi-v4",
                ffiContractHash = "c".repeat(64),
                snapshotSchemaVersion = 1U,
                minimumStorageSchemaVersion = 5U,
                currentStorageSchemaVersion = 10U,
            )
        assertTrue(clean.releaseReady)
        assertEquals("1700000000", clean.safeDiagnostics()["sourceDateEpoch"])
        assertFalse(clean.safeDiagnostics().values.any { it.contains("nsec1") })
        assertFalse(clean.copy(sourceDirty = BuildDirtyState.Dirty).releaseReady)
        assertFalse(clean.copy(sourceCommit = UNKNOWN_PROVENANCE).releaseReady)
        assertFalse(BuildInfo.unknown().releaseReady)
    }

    @Test
    fun identifiersAndCommandInputsValidateAndRedact() {
        val identityId = IdentityId.fromPublicKeyHex("01".repeat(32))
        assertEquals("01".repeat(32), identityId.value)
        assertFailsWith<IllegalArgumentException> { IdentityId.fromPublicKeyHex("AB".repeat(32)) }
        assertEquals(TEST_OPERATION_ID, OperationId.from(TEST_OPERATION_ID).value)
        assertFailsWith<IllegalArgumentException> { OperationId.from("contains space") }
        assertFailsWith<IllegalArgumentException> { OperationId.from("01890f3e-7b1c-4000-8000-000000000001") }
        assertFailsWith<IllegalArgumentException> { OperationId.from(TEST_OPERATION_ID.uppercase()) }
        assertFailsWith<IllegalArgumentException> { OperationId.from("01890f3e-7b1c-6000-8000-000000000001") }
        assertFailsWith<IllegalArgumentException> {
            RequestContext(OperationId.from(TEST_OPERATION_ID), SnapshotRevision(0UL), 0UL)
        }

        val secret = SecretKeyInput.from("nsec1boundedsecret")
        assertFalse(secret.toString().contains("nsec1boundedsecret"))
        assertEquals("nsec1boundedsecret", secret.take())
        assertFailsWith<IllegalStateException> { secret.take() }
    }

    @Test
    fun snapshotsRejectBrokenIdentityAndRevisionRelationships() {
        val snapshot = snapshot(revision = 2UL)
        assertEquals(snapshot.identities.single().id, snapshot.selectedIdentityId)
        assertFailsWith<IllegalArgumentException> {
            snapshot.copy(selectedIdentityId = IdentityId.fromPublicKeyHex("02".repeat(32)))
        }
        assertFailsWith<IllegalArgumentException> {
            ApplicationChange(snapshot, SnapshotRevision(2UL))
        }
        assertFailsWith<IllegalArgumentException> {
            ApplicationCommandResult.Committed(
                operationId = OperationId.from(TEST_OPERATION_ID),
                committedRevision = SnapshotRevision(1UL),
                snapshot = snapshot,
            )
        }
    }

    @Test
    fun lifecycleErrorAndRecoveryModelsAreExhaustive() {
        assertEquals(
            11,
            ApplicationLifecycle.entries
                .map(::lifecycleName)
                .distinct()
                .size,
        )
        assertEquals(
            5,
            SessionLifecycle.entries
                .map(::sessionName)
                .distinct()
                .size,
        )
        assertEquals(
            3,
            SignerAvailability.entries
                .map(::availabilityName)
                .distinct()
                .size,
        )
        assertEquals(
            5,
            RelayConnectionState.entries
                .map(::relayName)
                .distinct()
                .size,
        )
        assertEquals(
            5,
            ProfileLoadState.entries
                .map(::profileStateName)
                .distinct()
                .size,
        )
        assertEquals(
            23,
            ApplicationErrorCode.entries
                .map(::errorCodeName)
                .distinct()
                .size,
        )
        assertEquals(
            8,
            ApplicationErrorCategory.entries
                .map(::errorCategoryName)
                .distinct()
                .size,
        )
        assertEquals(
            9,
            RecoveryAction.entries
                .map(::recoveryName)
                .distinct()
                .size,
        )
    }

    @Test
    fun everyFoundationCommandHasAnExplicitRuntimeKind() {
        val id = IdentityId.fromPublicKeyHex("01".repeat(32))
        val context = RequestContext(OperationId.from(TEST_OPERATION_ID), SnapshotRevision(1UL), 1_000UL)
        val commands =
            listOf(
                ApplicationCommand.AcknowledgeGeneratedIdentity(RecoveryRequestId.from("recovery-1"), context),
                ApplicationCommand.CancelGeneratedIdentity(RecoveryRequestId.from("recovery-1")),
                ApplicationCommand.ImportLocalIdentity(SecretKeyInput.from("nsec1boundedsecret"), context),
                ApplicationCommand.SelectIdentity(id),
                ApplicationCommand.ActivateIdentity(id),
                ApplicationCommand.SignOut,
                ApplicationCommand.RefreshActiveProfile,
                ApplicationCommand.ConfirmIdentityRemoval(RemovalRequestId.from("removal-1"), context),
            )

        assertEquals(8, commands.map(::commandName).distinct().size)
        assertTrue(commands.none { it.toString().contains("nsec1boundedsecret") })
    }
}

private fun snapshot(revision: ULong): ApplicationSnapshot {
    val identity =
        IdentitySummary(
            id = IdentityId.fromPublicKeyHex("01".repeat(32)),
            npub = "npub1identity",
            displayLabel = "Identity",
            signer = SignerBindingSummary(SignerBindingKind.LocalKeyring, SignerAvailability.Available),
            createdAt = UnixSeconds(1),
            lastUsedAt = null,
        )
    return ApplicationSnapshot(
        revision = SnapshotRevision(revision),
        lifecycle = ApplicationLifecycle.Ready,
        lifecycleProblem = null,
        configuredRelays = listOf("wss://relay.example"),
        identities = listOf(identity),
        selectedIdentityId = identity.id,
        session = SessionLifecycle.SignedOut,
        sessionSubjectIdentityId = null,
        sessionProblem = null,
        activeIdentity = null,
        recoverableProblem = null,
    )
}

private fun lifecycleName(value: ApplicationLifecycle): String =
    when (value) {
        ApplicationLifecycle.Opening -> "opening"
        ApplicationLifecycle.CompatibilityChecking -> "compatibility"
        ApplicationLifecycle.AcquiringOwnership -> "ownership"
        ApplicationLifecycle.Migrating -> "migrating"
        ApplicationLifecycle.Recovering -> "recovering"
        ApplicationLifecycle.Ready -> "ready"
        ApplicationLifecycle.Degraded -> "degraded"
        ApplicationLifecycle.Blocked -> "blocked"
        ApplicationLifecycle.ShuttingDown -> "shutting-down"
        ApplicationLifecycle.Closed -> "closed"
        ApplicationLifecycle.Fatal -> "fatal"
    }

private fun sessionName(value: SessionLifecycle): String =
    when (value) {
        SessionLifecycle.SignedOut -> "signed-out"
        SessionLifecycle.Activating -> "activating"
        SessionLifecycle.Active -> "active"
        SessionLifecycle.SigningOut -> "signing-out"
        SessionLifecycle.Failed -> "failed"
    }

private fun availabilityName(value: SignerAvailability): String =
    when (value) {
        SignerAvailability.Available -> "available"
        SignerAvailability.CredentialMissing -> "credential-missing"
        SignerAvailability.StoreUnavailable -> "store-unavailable"
    }

private fun relayName(value: RelayConnectionState): String =
    when (value) {
        RelayConnectionState.Disconnected -> "disconnected"
        RelayConnectionState.Connecting -> "connecting"
        RelayConnectionState.Connected -> "connected"
        RelayConnectionState.Degraded -> "degraded"
        RelayConnectionState.Error -> "error"
    }

private fun profileStateName(value: ProfileLoadState): String =
    when (value) {
        ProfileLoadState.Empty -> "empty"
        ProfileLoadState.Loading -> "loading"
        ProfileLoadState.Cached -> "cached"
        ProfileLoadState.Fresh -> "fresh"
        ProfileLoadState.Error -> "error"
    }

private fun errorCodeName(value: ApplicationErrorCode): String = value.name

private fun errorCategoryName(value: ApplicationErrorCategory): String = value.name

private fun recoveryName(value: RecoveryAction): String = value.name

private fun commandName(value: ApplicationCommand): String =
    when (value) {
        is ApplicationCommand.AcknowledgeGeneratedIdentity -> "acknowledge-generated"
        is ApplicationCommand.CancelGeneratedIdentity -> "cancel-generated"
        is ApplicationCommand.ImportLocalIdentity -> "import"
        is ApplicationCommand.SelectIdentity -> "select"
        is ApplicationCommand.ActivateIdentity -> "activate"
        ApplicationCommand.SignOut -> "sign-out"
        ApplicationCommand.RefreshActiveProfile -> "refresh-profile"
        is ApplicationCommand.ConfirmIdentityRemoval -> "confirm-removal"
    }

private const val TEST_OPERATION_ID = "01890f3e-7b1c-7000-8000-000000000001"
