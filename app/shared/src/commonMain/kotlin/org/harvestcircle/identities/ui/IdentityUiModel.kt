package org.harvestcircle.identities.ui

import org.harvestcircle.application.ActiveIdentity
import org.harvestcircle.application.ApplicationErrorCode
import org.harvestcircle.application.HarvestCirclePresenterState
import org.harvestcircle.application.HarvestCircleRoute
import org.harvestcircle.application.IdentityEntryMode
import org.harvestcircle.application.IdentitySummary
import org.harvestcircle.application.ProfileLoadState
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.RelayConnectionState
import org.harvestcircle.application.RemovalImpactState
import org.harvestcircle.application.RemovalStatus
import org.harvestcircle.application.SessionLifecycle

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
    val session: SessionLifecycle,
    val busy: Boolean,
    val problem: String?,
    val importGuidance: String?,
    val recoveryAction: RecoveryAction,
)

fun HarvestCirclePresenterState.toUiModel(): HarvestCircleUiModel {
    val selectedPublicKeyHex = snapshot.selectedIdentityId?.value
    val activePublicKeyHex =
        snapshot.activeIdentity
            ?.identity
            ?.id
            ?.value
    val identities =
        snapshot.identities.map {
            it.toUiModel(
                selected = it.id.value == selectedPublicKeyHex,
                active = it.id.value == activePublicKeyHex,
            )
        }
    return HarvestCircleUiModel(
        route = route,
        identities = identities,
        activeIdentity = snapshot.activeIdentity?.toUiModel(selectedPublicKeyHex),
        configuredRelays = snapshot.configuredRelays.map { it.url },
        importDraft = importDraft,
        generatedKeyBackup =
            generatedKeyBackup?.let { backup ->
                backup.revealNsecOrNull()?.let { nsec ->
                    GeneratedKeyBackupUiModel(npub = backup.npub, nsec = nsec)
                }
            },
        pendingRemovalPublicKeyHex = pendingRemovalIdentityId?.value,
        removalImpact = removalImpact,
        removalStatus = removalStatus,
        lastRemovedPublicKeyHex = lastRemovedIdentityId?.value,
        identityChooserVisible = identityChooserVisible,
        identityEntryMode = identityEntryMode,
        session = snapshot.session,
        busy = busy,
        problem =
            problem
                ?: snapshot.recoverableProblem?.safeMessage
                ?: snapshot.sessionProblem?.safeMessage
                ?: snapshot.lifecycleProblem?.safeMessage,
        importGuidance = importGuidance(lastProblem?.code, lastProblem?.recoveryAction ?: RecoveryAction.None),
        recoveryAction = lastProblem?.recoveryAction ?: RecoveryAction.None,
    )
}

private fun importGuidance(
    code: ApplicationErrorCode?,
    recoveryAction: RecoveryAction,
): String? =
    when {
        code == ApplicationErrorCode.InvalidSecretKey -> "Enter a valid nsec or 64-character hexadecimal secret key."
        code == ApplicationErrorCode.IdentityAlreadyExists -> "This Nostr identity is already saved."
        code == ApplicationErrorCode.CredentialMissing || recoveryAction == RecoveryAction.RepairCredential ->
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

private fun IdentitySummary.toUiModel(
    selected: Boolean,
    active: Boolean = false,
) = IdentityUiModel(
    publicKeyHex = id.value,
    npub = npub,
    shortNpub = shortenNpub(npub),
    label = displayLabel.ifBlank { shortenNpub(npub) },
    signerAvailability =
        signer.availability.name
            .lowercase()
            .replace('_', ' '),
    selected = selected,
    active = active,
)

private fun ActiveIdentity.toUiModel(selectedPublicKeyHex: String?) =
    ActiveIdentityUiModel(
        identity =
            identity.toUiModel(
                selected = identity.id.value == selectedPublicKeyHex,
                active = true,
            ),
        heading =
            profile?.displayName?.takeIf(String::isNotBlank)
                ?: profile?.name?.takeIf(String::isNotBlank)
                ?: identity.displayLabel.ifBlank { shortenNpub(identity.npub) },
        relayState = relays.state.toDisplayText(),
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

private fun RelayConnectionState.toDisplayText(): String = name.lowercase().replace('_', ' ')

private fun ProfileLoadState.toDisplayText(): String = name.lowercase().replace('_', ' ')
