package org.radroots.studio.accounts.ui

import org.radroots.studio.application.StudioStoreState
import org.radroots.studio.application.StudioRoute
import org.radroots.studio.application.AccountEntryMode
import org.radroots.studio.ffi.AccountDto
import org.radroots.studio.ffi.ActiveAccountDto
import org.radroots.studio.ffi.ProfileLoadStateDto
import org.radroots.studio.ffi.RelayConnectionStateDto
import org.radroots.studio.ffi.SessionStateDto

data class AccountUiModel(
    val publicKeyHex: String,
    val npub: String,
    val shortNpub: String,
    val label: String,
    val keyAvailability: String,
    val selected: Boolean,
)

data class ProfileUiModel(
    val name: String,
    val displayName: String,
    val nip05: String,
    val about: String,
    val picture: String,
)

data class ActiveAccountUiModel(
    val account: AccountUiModel,
    val heading: String,
    val relayState: String,
    val profileState: String,
    val profile: ProfileUiModel,
)

class GeneratedKeyBackupUiModel(
    val npub: String,
    val nsec: String,
) {
    override fun toString(): String = "GeneratedKeyBackupUiModel(npub=$npub, nsec=[REDACTED])"
}

data class StudioUiModel(
    val route: StudioRoute,
    val accounts: List<AccountUiModel>,
    val activeAccount: ActiveAccountUiModel?,
    val configuredRelays: List<String>,
    val importDraft: String,
    val generatedKeyBackup: GeneratedKeyBackupUiModel?,
    val pendingRemovalPublicKeyHex: String?,
    val accountChooserVisible: Boolean,
    val accountEntryMode: AccountEntryMode,
    val session: SessionStateDto,
    val busy: Boolean,
    val problem: String?,
)

fun StudioStoreState.toUiModel(): StudioUiModel {
    val selectedPublicKeyHex = snapshot.selectedPublicKeyHex
    val accounts = snapshot.accounts.map { it.toUiModel(it.publicKeyHex == selectedPublicKeyHex) }
    return StudioUiModel(
        route = route,
        accounts = accounts,
        activeAccount = snapshot.activeAccount?.toUiModel(selectedPublicKeyHex),
        configuredRelays = snapshot.configuredRelays,
        importDraft = importDraft,
        generatedKeyBackup = generatedKeyBackup?.let {
            GeneratedKeyBackupUiModel(npub = it.npub, nsec = it.revealNsec())
        },
        pendingRemovalPublicKeyHex = pendingRemovalPublicKeyHex,
        accountChooserVisible = accountChooserVisible,
        accountEntryMode = accountEntryMode,
        session = snapshot.session,
        busy = busy,
        problem = problem
            ?: snapshot.recoverableProblem?.message
            ?: snapshot.sessionError?.message
            ?: snapshot.lifecycleError?.message,
    )
}

fun shortenNpub(npub: String): String =
    if (npub.length <= 24) npub else "${npub.take(14)}…${npub.takeLast(8)}"

private fun AccountDto.toUiModel(selected: Boolean) = AccountUiModel(
    publicKeyHex = publicKeyHex,
    npub = npub,
    shortNpub = shortenNpub(npub),
    label = displayLabel.ifBlank { shortenNpub(npub) },
    keyAvailability = keyAvailability.name.lowercase().replace('_', ' '),
    selected = selected,
)

private fun ActiveAccountDto.toUiModel(selectedPublicKeyHex: String?) = ActiveAccountUiModel(
    account = account.toUiModel(account.publicKeyHex == selectedPublicKeyHex),
    heading = profile?.displayName?.takeIf(String::isNotBlank)
        ?: profile?.name?.takeIf(String::isNotBlank)
        ?: account.displayLabel.ifBlank { shortenNpub(account.npub) },
    relayState = relayState.toDisplayText(),
    profileState = profileState.toDisplayText(),
    profile = ProfileUiModel(
        name = profile?.name.orEmpty(),
        displayName = profile?.displayName.orEmpty(),
        nip05 = profile?.nip05.orEmpty(),
        about = profile?.about.orEmpty(),
        picture = profile?.picture.orEmpty(),
    ),
)

private fun RelayConnectionStateDto.toDisplayText(): String =
    name.lowercase().replace('_', ' ')

private fun ProfileLoadStateDto.toDisplayText(): String =
    name.lowercase().replace('_', ' ')
