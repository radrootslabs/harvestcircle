package org.harvestcircle.identities.ui

import org.harvestcircle.application.HarvestCircleRoute
import org.harvestcircle.application.HarvestCircleStoreState
import org.harvestcircle.application.IdentityEntryMode
import org.harvestcircle.application.RemovalImpactState
import org.harvestcircle.application.RemovalStatus
import org.harvestcircle.ffi.ActiveIdentityDto
import org.harvestcircle.ffi.IdentityDto
import org.harvestcircle.ffi.ProfileLoadStateDto
import org.harvestcircle.ffi.RelayConnectionStateDto
import org.harvestcircle.ffi.SessionStateDto
import org.harvestcircle.ffi.WireErrorCode
import org.harvestcircle.ffi.WireRecoveryAction

data class IdentityUiModel(
    val publicKeyHex: String,
    val npub: String,
    val shortNpub: String,
    val label: String,
    val signerAvailability: String,
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

data class ActiveIdentityUiModel(
    val identity: IdentityUiModel,
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

data class HarvestCircleUiModel(
    val route: HarvestCircleRoute,
    val identities: List<IdentityUiModel>,
    val activeIdentity: ActiveIdentityUiModel?,
    val configuredRelays: List<String>,
    val importDraft: String,
    val generatedKeyBackup: GeneratedKeyBackupUiModel?,
    val pendingRemovalPublicKeyHex: String?,
    val removalImpact: RemovalImpactState?,
    val removalStatus: RemovalStatus,
    val lastRemovedPublicKeyHex: String?,
    val identityChooserVisible: Boolean,
    val identityEntryMode: IdentityEntryMode,
    val session: SessionStateDto,
    val busy: Boolean,
    val problem: String?,
    val importGuidance: String?,
    val recoveryAction: WireRecoveryAction,
)

fun HarvestCircleStoreState.toUiModel(): HarvestCircleUiModel {
    val selectedPublicKeyHex = snapshot.selectedPublicKeyHex
    val activePublicKeyHex = snapshot.activeIdentity?.identity?.publicKeyHex
    val identities =
        snapshot.identities.map {
            it.toUiModel(
                selected = it.publicKeyHex == selectedPublicKeyHex,
                active = it.publicKeyHex == activePublicKeyHex,
            )
        }
    return HarvestCircleUiModel(
        route = route,
        identities = identities,
        activeIdentity = snapshot.activeIdentity?.toUiModel(selectedPublicKeyHex),
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
        identityChooserVisible = identityChooserVisible,
        identityEntryMode = identityEntryMode,
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
        code == WireErrorCode.IDENTITY_ALREADY_EXISTS -> "This Nostr identity is already saved."
        code == WireErrorCode.CREDENTIAL_MISSING || recoveryAction == WireRecoveryAction.REPAIR_CREDENTIAL ->
            "This saved identity is missing its local credential. Re-enter its secret key to repair it."
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

private fun IdentityDto.toUiModel(
    selected: Boolean,
    active: Boolean = false,
) = IdentityUiModel(
    publicKeyHex = publicKeyHex,
    npub = npub,
    shortNpub = shortenNpub(npub),
    label = displayLabel.ifBlank { shortenNpub(npub) },
    signerAvailability = signerAvailability.name.lowercase().replace('_', ' '),
    selected = selected,
    active = active,
)

private fun ActiveIdentityDto.toUiModel(selectedPublicKeyHex: String?) =
    ActiveIdentityUiModel(
        identity =
            identity.toUiModel(
                selected = identity.publicKeyHex == selectedPublicKeyHex,
                active = true,
            ),
        heading =
            profile?.displayName?.takeIf(String::isNotBlank)
                ?: profile?.name?.takeIf(String::isNotBlank)
                ?: identity.displayLabel.ifBlank { shortenNpub(identity.npub) },
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
