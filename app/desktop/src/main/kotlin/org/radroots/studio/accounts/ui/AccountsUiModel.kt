package org.radroots.studio.accounts.ui

import org.radroots.studio.application.AccountEntryMode
import org.radroots.studio.application.RemovalImpactState
import org.radroots.studio.application.RemovalStatus
import org.radroots.studio.application.StudioRoute
import org.radroots.studio.application.StudioStoreState
import org.radroots.studio.ffi.AccountDto
import org.radroots.studio.ffi.ActiveAccountDto
import org.radroots.studio.ffi.ProfileLoadStateDto
import org.radroots.studio.ffi.RelayConnectionStateDto
import org.radroots.studio.ffi.SessionStateDto
import org.radroots.studio.ffi.WireErrorCode
import org.radroots.studio.ffi.WireRecoveryAction

data class AccountUiModel(
    val publicKeyHex: String,
    val npub: String,
    val shortNpub: String,
    val label: String,
    val keyAvailability: String,
    val selected: Boolean,
    val active: Boolean,
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
    val removalImpact: RemovalImpactState?,
    val removalStatus: RemovalStatus,
    val lastRemovedPublicKeyHex: String?,
    val accountChooserVisible: Boolean,
    val accountEntryMode: AccountEntryMode,
    val session: SessionStateDto,
    val busy: Boolean,
    val problem: String?,
    val importGuidance: String?,
    val recoveryAction: WireRecoveryAction,
)

fun StudioStoreState.toUiModel(): StudioUiModel {
    val selectedPublicKeyHex = snapshot.selectedPublicKeyHex
    val activePublicKeyHex = snapshot.activeAccount?.account?.publicKeyHex
    val accounts =
        snapshot.accounts.map {
            it.toUiModel(
                selected = it.publicKeyHex == selectedPublicKeyHex,
                active = it.publicKeyHex == activePublicKeyHex,
            )
        }
    return StudioUiModel(
        route = route,
        accounts = accounts,
        activeAccount = snapshot.activeAccount?.toUiModel(selectedPublicKeyHex),
        configuredRelays = snapshot.configuredRelays,
        importDraft = importDraft,
        generatedKeyBackup =
            generatedKeyBackup?.let {
                GeneratedKeyBackupUiModel(npub = it.npub, nsec = it.revealNsec())
            },
        pendingRemovalPublicKeyHex = pendingRemovalPublicKeyHex,
        removalImpact = removalImpact,
        removalStatus = removalStatus,
        lastRemovedPublicKeyHex = lastRemovedPublicKeyHex,
        accountChooserVisible = accountChooserVisible,
        accountEntryMode = accountEntryMode,
        session = snapshot.session,
        busy = busy,
        problem =
            problem
                ?: snapshot.recoverableProblem?.message
                ?: snapshot.sessionError?.message
                ?: snapshot.lifecycleError?.message,
        importGuidance = importGuidance(lastFailureCode, recoveryAction),
        recoveryAction = recoveryAction,
    )
}

private fun importGuidance(
    code: WireErrorCode?,
    recoveryAction: WireRecoveryAction,
): String? =
    when {
        code == WireErrorCode.INVALID_SECRET_KEY -> "Enter a valid nsec or 64-character hexadecimal secret key."
        code == WireErrorCode.ACCOUNT_ALREADY_EXISTS -> "This Nostr account is already saved."
        code == WireErrorCode.CREDENTIAL_MISSING || recoveryAction == WireRecoveryAction.REPAIR_CREDENTIAL ->
            "This saved account is missing its local credential. Re-enter its secret key to repair it."
        else -> null
    }

private const val SHORT_NPUB_MAX_LENGTH = 24
private const val SHORT_NPUB_PREFIX_LENGTH = 14
private const val SHORT_NPUB_SUFFIX_LENGTH = 8

fun shortenNpub(npub: String): String =
    if (npub.length <= SHORT_NPUB_MAX_LENGTH) {
        npub
    } else {
        "${npub.take(SHORT_NPUB_PREFIX_LENGTH)}…${npub.takeLast(SHORT_NPUB_SUFFIX_LENGTH)}"
    }

private fun AccountDto.toUiModel(
    selected: Boolean,
    active: Boolean = false,
) = AccountUiModel(
    publicKeyHex = publicKeyHex,
    npub = npub,
    shortNpub = shortenNpub(npub),
    label = displayLabel.ifBlank { shortenNpub(npub) },
    keyAvailability = keyAvailability.name.lowercase().replace('_', ' '),
    selected = selected,
    active = active,
)

private fun ActiveAccountDto.toUiModel(selectedPublicKeyHex: String?) =
    ActiveAccountUiModel(
        account =
            account.toUiModel(
                selected = account.publicKeyHex == selectedPublicKeyHex,
                active = true,
            ),
        heading =
            profile?.displayName?.takeIf(String::isNotBlank)
                ?: profile?.name?.takeIf(String::isNotBlank)
                ?: account.displayLabel.ifBlank { shortenNpub(account.npub) },
        relayState = relayState.toDisplayText(),
        profileState = profileState.toDisplayText(),
        profile =
            ProfileUiModel(
                name = profile?.name.orEmpty(),
                displayName = profile?.displayName.orEmpty(),
                nip05 = profile?.nip05.orEmpty(),
                about = profile?.about.orEmpty(),
                picture = profile?.picture.orEmpty(),
            ),
    )

private fun RelayConnectionStateDto.toDisplayText(): String = name.lowercase().replace('_', ' ')

private fun ProfileLoadStateDto.toDisplayText(): String = name.lowercase().replace('_', ' ')
