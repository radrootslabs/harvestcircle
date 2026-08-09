package org.radroots.harvestcircle.accounts.ui

import org.radroots.harvestcircle.application.GeneratedKeyBackup
import org.radroots.harvestcircle.application.HarvestCircleStoreState
import org.radroots.harvestcircle.ffi.AccountDto
import org.radroots.harvestcircle.ffi.ActiveAccountDto
import org.radroots.harvestcircle.ffi.AppLifecycleDto
import org.radroots.harvestcircle.ffi.AppSnapshotDto
import org.radroots.harvestcircle.ffi.KeyAvailabilityDto
import org.radroots.harvestcircle.ffi.ProfileDto
import org.radroots.harvestcircle.ffi.ProfileLoadStateDto
import org.radroots.harvestcircle.ffi.RelayConnectionStateDto
import org.radroots.harvestcircle.ffi.SessionStateDto
import org.radroots.harvestcircle.ffi.SignerKindDto
import org.radroots.harvestcircle.ffi.WireErrorCode
import org.radroots.harvestcircle.ffi.WireRecoveryAction
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class AccountsUiModelTest {
    @Test
    fun mapsPublicNostrIdentityAndProfileState() {
        val account = account()
        val snapshot =
            snapshot(
                account = account,
                active =
                    ActiveAccountDto(
                        account = account,
                        relayState = RelayConnectionStateDto.CONNECTED,
                        profileState = ProfileLoadStateDto.FRESH,
                        profile = ProfileDto("alice", "Alice", "alice@example.com", "Farmer", "https://example.com/a.png"),
                    ),
            )

        val model = HarvestCircleStoreState(snapshot).toUiModel()

        assertEquals("Alice", model.activeAccount?.heading)
        assertEquals("connected", model.activeAccount?.relayState)
        assertEquals("fresh", model.activeAccount?.profileState)
        assertEquals("alice@example.com", model.activeAccount?.profile?.nip05)
        assertEquals(listOf("ws://localhost:8080"), model.configuredRelays)
        assertFalse(model.accountChooserVisible)
        assertFalse(
            model.accounts
                .single()
                .label
                .contains("server", ignoreCase = true),
        )
        assertTrue(model.accounts.single().selected)
        assertTrue(model.accounts.single().active)
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
            "This saved account is missing its local credential. Re-enter its secret key to repair it.",
            repair.importGuidance,
        )
    }
}

private fun snapshot(
    account: AccountDto? = null,
    active: ActiveAccountDto? = null,
) = AppSnapshotDto(
    revision = 1UL,
    lifecycle = AppLifecycleDto.READY,
    lifecycleError = null,
    configuredRelays = listOf("ws://localhost:8080"),
    accounts = listOfNotNull(account),
    selectedPublicKeyHex = account?.publicKeyHex,
    session = if (active == null) SessionStateDto.SIGNED_OUT else SessionStateDto.ACTIVE,
    sessionSubjectPublicKeyHex = active?.account?.publicKeyHex,
    sessionError = null,
    activeAccount = active,
    recoverableProblem = null,
)

private fun account() =
    AccountDto(
        publicKeyHex = "12".repeat(32),
        npub = "npub1abcdefghijklmnopqrstuvwxyz1234567890",
        displayLabel = "Alice",
        signerKind = SignerKindDto.LOCAL_SECRET,
        keyAvailability = KeyAvailabilityDto.AVAILABLE,
        createdAtSeconds = 1,
        lastUsedAtSeconds = null,
    )
