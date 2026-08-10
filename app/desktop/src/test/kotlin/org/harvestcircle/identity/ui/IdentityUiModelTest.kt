package org.harvestcircle.identities.ui

import org.harvestcircle.application.GeneratedKeyBackup
import org.harvestcircle.application.HarvestCircleStoreState
import org.harvestcircle.ffi.ActiveIdentityDto
import org.harvestcircle.ffi.AppLifecycleDto
import org.harvestcircle.ffi.AppSnapshotDto
import org.harvestcircle.ffi.IdentityDto
import org.harvestcircle.ffi.ProfileDto
import org.harvestcircle.ffi.ProfileLoadStateDto
import org.harvestcircle.ffi.RelayConnectionStateDto
import org.harvestcircle.ffi.SessionStateDto
import org.harvestcircle.ffi.SignerAvailabilityDto
import org.harvestcircle.ffi.SignerBindingKindDto
import org.harvestcircle.ffi.WireErrorCode
import org.harvestcircle.ffi.WireRecoveryAction
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
                    ActiveIdentityDto(
                        identity = identity,
                        relayState = RelayConnectionStateDto.CONNECTED,
                        profileState = ProfileLoadStateDto.FRESH,
                        profile = ProfileDto("alice", "Alice", "alice@example.com", "Farmer", "https://example.com/a.png"),
                    ),
            )

        val model = HarvestCircleStoreState(snapshot).toUiModel()

        assertEquals("Alice", model.activeIdentity?.heading)
        assertEquals("connected", model.activeIdentity?.relayState)
        assertEquals("fresh", model.activeIdentity?.profileState)
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
            HarvestCircleStoreState(
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
            HarvestCircleStoreState(
                snapshot = snapshot(),
                lastFailureCode = WireErrorCode.INVALID_SECRET_KEY,
            ).toUiModel()
        val repair =
            HarvestCircleStoreState(
                snapshot = snapshot(),
                lastFailureCode = WireErrorCode.CREDENTIAL_MISSING,
                recoveryAction = WireRecoveryAction.REPAIR_CREDENTIAL,
            ).toUiModel()

        assertEquals("Enter a valid nsec or 64-character hexadecimal secret key.", invalid.importGuidance)
        assertEquals(
            "This saved identity is missing its local credential. Re-enter its secret key to repair it.",
            repair.importGuidance,
        )
    }
}

private fun snapshot(
    identity: IdentityDto? = null,
    active: ActiveIdentityDto? = null,
) = AppSnapshotDto(
    revision = 1UL,
    lifecycle = AppLifecycleDto.READY,
    lifecycleError = null,
    configuredRelays = listOf("ws://localhost:8080"),
    identities = listOfNotNull(identity),
    selectedPublicKeyHex = identity?.publicKeyHex,
    session = if (active == null) SessionStateDto.SIGNED_OUT else SessionStateDto.ACTIVE,
    sessionSubjectPublicKeyHex = active?.identity?.publicKeyHex,
    sessionError = null,
    activeIdentity = active,
    recoverableProblem = null,
)

private fun identity() =
    IdentityDto(
        publicKeyHex = "12".repeat(32),
        npub = "npub1abcdefghijklmnopqrstuvwxyz1234567890",
        displayLabel = "Alice",
        signerBindingKind = SignerBindingKindDto.LOCAL_KEYRING,
        signerAvailability = SignerAvailabilityDto.AVAILABLE,
        createdAtSeconds = 1,
        lastUsedAtSeconds = null,
    )
