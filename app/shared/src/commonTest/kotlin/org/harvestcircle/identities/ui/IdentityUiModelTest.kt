package org.harvestcircle.identities.ui

import org.harvestcircle.application.ActiveIdentity
import org.harvestcircle.application.ApplicationErrorCategory
import org.harvestcircle.application.ApplicationErrorCode
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.application.ApplicationProblem
import org.harvestcircle.application.ApplicationSnapshot
import org.harvestcircle.application.GeneratedKeyBackup
import org.harvestcircle.application.HarvestCirclePresenterState
import org.harvestcircle.application.IdentityId
import org.harvestcircle.application.IdentitySummary
import org.harvestcircle.application.ProfileLoadState
import org.harvestcircle.application.ProfileSummary
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.RelayConnectionState
import org.harvestcircle.application.RelayDestination
import org.harvestcircle.application.RelayEndpoint
import org.harvestcircle.application.RelaySummary
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.SignerAvailability
import org.harvestcircle.application.SignerBindingKind
import org.harvestcircle.application.SignerBindingSummary
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.application.UnixSeconds
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class IdentityUiModelTest {
    @Test
    fun mapsPublicNostrIdentityAndProfileState() {
        val identity = identity()
        val snapshot =
            snapshot(
                identity = identity,
                active =
                    ActiveIdentity(
                        identity = identity,
                        relays = RelaySummary(listOf(localRelay()), RelayConnectionState.Connected),
                        profileState = ProfileLoadState.Fresh,
                        profile = ProfileSummary("alice", "Alice", "alice@example.com", "Farmer", "https://example.com/a.png"),
                    ),
            )

        val model = HarvestCirclePresenterState(snapshot).toUiModel()

        assertEquals("Alice", model.activeIdentity?.heading)
        assertEquals(RelayConnectionState.Connected, model.activeIdentity?.relayState)
        assertEquals(ProfileLoadState.Fresh, model.activeIdentity?.profileState)
        assertEquals(SignerAvailability.Available, model.identities.single().signerAvailability)
        assertEquals("alice@example.com", model.activeIdentity?.profile?.nip05)
        assertEquals(listOf("ws://localhost:8080"), model.configuredRelays)
        assertFalse(model.identityChooserVisible)
        assertFalse(
            model.identities
                .single()
                .label
                .contains("server", ignoreCase = true),
        )
        assertTrue(model.identities.single().selected)
        assertTrue(model.identities.single().active)
    }

    @Test
    fun mapsSafeProblemAndTransientBackupSeparatelyFromSnapshot() {
        val state =
            HarvestCirclePresenterState(
                snapshot = snapshot(),
                generatedKeyBackup = GeneratedKeyBackup("npub1generated", "nsec1generated"),
                problem = "Try again.",
            )

        val model = state.toUiModel()

        assertEquals("Try again.", model.problem)
        assertEquals("nsec1generated", model.generatedKeyBackup?.nsec)
        assertNull(state.snapshot.recoverableProblem)
    }

    @Test
    fun shortensOnlyLongNpubValues() {
        assertEquals("npub1short", shortenNpub("npub1short"))
        assertEquals("npub1abcdefghi…34567890", shortenNpub("npub1abcdefghijklmnopqrstuvwxyz1234567890"))
    }

    @Test
    fun mapsTypedImportFailuresToSpecificRepairGuidance() {
        val invalid =
            HarvestCirclePresenterState(
                snapshot = snapshot(),
                lastProblem = problem(ApplicationErrorCode.InvalidSecretKey),
            ).toUiModel()
        val repair =
            HarvestCirclePresenterState(
                snapshot = snapshot(),
                lastProblem = problem(ApplicationErrorCode.CredentialMissing, RecoveryAction.RepairCredential),
            ).toUiModel()

        assertEquals("Enter a valid nsec or 64-character hexadecimal secret key.", invalid.importGuidance)
        assertEquals(
            "This saved identity is missing its local credential. Re-enter its secret key to repair it.",
            repair.importGuidance,
        )
    }
}

private fun snapshot(
    identity: IdentitySummary? = null,
    active: ActiveIdentity? = null,
) = ApplicationSnapshot(
    revision = SnapshotRevision(1UL),
    lifecycle = ApplicationLifecycle.Ready,
    lifecycleProblem = null,
    configuredRelays = listOf(localRelay()),
    identities = listOfNotNull(identity),
    selectedIdentityId = identity?.id,
    session = if (active == null) SessionLifecycle.SignedOut else SessionLifecycle.Active,
    sessionSubjectIdentityId = active?.identity?.id,
    sessionProblem = null,
    activeIdentity = active,
    recoverableProblem = null,
)

private fun localRelay() =
    RelayEndpoint(
        url = "ws://localhost:8080",
        destination = RelayDestination.Local,
        read = true,
        write = true,
    )

private fun identity() =
    IdentitySummary(
        id = IdentityId.fromPublicKeyHex("12".repeat(32)),
        npub = "npub1abcdefghijklmnopqrstuvwxyz1234567890",
        displayLabel = "Alice",
        signer = SignerBindingSummary(SignerBindingKind.LocalKeyring, SignerAvailability.Available),
        createdAt = UnixSeconds(1),
        lastUsedAt = null,
    )

private fun problem(
    code: ApplicationErrorCode,
    recoveryAction: RecoveryAction = RecoveryAction.None,
) = ApplicationProblem(
    code = code,
    category = ApplicationErrorCategory.Input,
    retryable = false,
    recoveryAction = recoveryAction,
    operationId = null,
    safeMessage = "Safe problem.",
)
