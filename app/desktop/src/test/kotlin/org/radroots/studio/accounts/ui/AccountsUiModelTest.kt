package org.radroots.studio.accounts.ui

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import org.radroots.studio.application.GeneratedKeyBackup
import org.radroots.studio.application.StudioStoreState
import org.radroots.studio.ffi.AccountDto
import org.radroots.studio.ffi.ActiveAccountDto
import org.radroots.studio.ffi.AppLifecycleDto
import org.radroots.studio.ffi.AppSnapshotDto
import org.radroots.studio.ffi.KeyAvailabilityDto
import org.radroots.studio.ffi.ProfileDto
import org.radroots.studio.ffi.ProfileLoadStateDto
import org.radroots.studio.ffi.RelayConnectionStateDto
import org.radroots.studio.ffi.SessionStateDto
import org.radroots.studio.ffi.SignerKindDto

class AccountsUiModelTest {
    @Test
    fun mapsPublicNostrIdentityAndProfileState() {
        val account = account()
        val snapshot = snapshot(
            account = account,
            active = ActiveAccountDto(
                account = account,
                relayState = RelayConnectionStateDto.CONNECTED,
                profileState = ProfileLoadStateDto.FRESH,
                profile = ProfileDto("alice", "Alice", "alice@example.com", "Farmer", "https://example.com/a.png"),
            ),
        )

        val model = StudioStoreState(snapshot).toUiModel()

        assertEquals("Alice", model.activeAccount?.heading)
        assertEquals("connected", model.activeAccount?.relayState)
        assertEquals("fresh", model.activeAccount?.profileState)
        assertEquals("alice@example.com", model.activeAccount?.profile?.nip05)
        assertEquals(listOf("ws://localhost:8080"), model.configuredRelays)
        assertFalse(model.accounts.single().label.contains("server", ignoreCase = true))
    }

    @Test
    fun mapsSafeProblemAndTransientBackupSeparatelyFromSnapshot() {
        val state = StudioStoreState(
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

private fun account() = AccountDto(
    publicKeyHex = "12".repeat(32),
    npub = "npub1abcdefghijklmnopqrstuvwxyz1234567890",
    displayLabel = "Alice",
    signerKind = SignerKindDto.LOCAL_SECRET,
    keyAvailability = KeyAvailabilityDto.AVAILABLE,
    createdAtSeconds = 1,
    lastUsedAtSeconds = null,
)
