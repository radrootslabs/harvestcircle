package org.harvestcircle.application

enum class ApplicationLifecycle {
    Opening,
    CompatibilityChecking,
    AcquiringOwnership,
    Migrating,
    Recovering,
    Ready,
    Degraded,
    Blocked,
    ShuttingDown,
    Closed,
    Fatal,
}

enum class SessionLifecycle {
    SignedOut,
    Activating,
    Active,
    SigningOut,
    Failed,
}

enum class SignerAvailability {
    Available,
    CredentialMissing,
    StoreUnavailable,
    NotRequired,
}

sealed interface SignerBindingKind {
    data object LocalKeyring : SignerBindingKind

    data class Unsupported(
        val protocol: String,
    ) : SignerBindingKind {
        init {
            requireSafeText(protocol, "Signer protocol", 64)
        }
    }
}

data class SignerBindingSummary(
    val kind: SignerBindingKind,
    val availability: SignerAvailability,
)

data class IdentitySummary(
    val id: IdentityId,
    val npub: String,
    val displayLabel: String,
    val signer: SignerBindingSummary,
    val createdAt: UnixSeconds,
    val lastUsedAt: UnixSeconds?,
) {
    init {
        requireSafeText(npub, "Nostr public identity", 128)
        requireSafeText(displayLabel, "Identity display label", 128)
        require(lastUsedAt == null || lastUsedAt.value >= createdAt.value) {
            "Identity last-used time precedes creation"
        }
    }
}

enum class RelayConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Degraded,
    Error,
}

data class RelaySummary(
    val destinations: List<String>,
    val state: RelayConnectionState,
) {
    init {
        require(destinations.distinct() == destinations) { "Relay destinations must be unique" }
        destinations.forEach { requireSafeText(it, "Relay destination", 2048) }
    }
}

enum class ProfileLoadState {
    Empty,
    Loading,
    Cached,
    Fresh,
    Error,
}

data class ProfileSummary(
    val name: String?,
    val displayName: String?,
    val nip05: String?,
    val about: String?,
    val picture: String?,
) {
    init {
        validateOptional(name, "Profile name", 256)
        validateOptional(displayName, "Profile display name", 256)
        validateOptional(nip05, "Profile NIP-05 identifier", 320)
        validateOptional(about, "Profile about text", 4096)
        validateOptional(picture, "Profile picture URL", 2048)
    }
}

data class ActiveIdentity(
    val identity: IdentitySummary,
    val relays: RelaySummary,
    val profileState: ProfileLoadState,
    val profile: ProfileSummary?,
)

enum class ApplicationErrorCode {
    InvalidPublicKey,
    InvalidSecretKey,
    InvalidIdentityMetadata,
    InvalidProfileMetadata,
    InvalidApplicationState,
    IdentityAlreadyExists,
    IdentityNotFound,
    KeyringUnavailable,
    CredentialMissing,
    StorageUnavailable,
    StorageCorrupt,
    StorageQuarantined,
    StorageBackupInvalid,
    UnsupportedSchemaVersion,
    RepairUnauthorized,
    PendingOperationRecoveryRequired,
    InvalidRelayConfiguration,
    RelayConnectionFailed,
    ProfileRefreshFailed,
    ObserverRegistrationFailed,
    NativeLibraryLoadFailed,
    CompatibilityMismatch,
    Internal,
}

enum class ApplicationErrorCategory {
    Input,
    Conflict,
    Credential,
    Storage,
    Network,
    Lifecycle,
    Compatibility,
    Internal,
}

enum class RecoveryAction {
    None,
    Retry,
    RepairCredential,
    Authenticate,
    RepairStorage,
    RestoreBackup,
    CheckConfiguration,
    RestartApplication,
    UpdateApplication,
}

data class ApplicationProblem(
    val code: ApplicationErrorCode,
    val category: ApplicationErrorCategory,
    val retryable: Boolean,
    val recoveryAction: RecoveryAction,
    val operationId: OperationId?,
    val safeMessage: String,
) {
    init {
        requireSafeText(safeMessage, "Safe error message", 512)
    }
}

data class ApplicationSnapshot(
    val revision: SnapshotRevision,
    val lifecycle: ApplicationLifecycle,
    val lifecycleProblem: ApplicationProblem?,
    val configuredRelays: List<String>,
    val identities: List<IdentitySummary>,
    val selectedIdentityId: IdentityId?,
    val session: SessionLifecycle,
    val sessionSubjectIdentityId: IdentityId?,
    val sessionProblem: ApplicationProblem?,
    val activeIdentity: ActiveIdentity?,
    val recoverableProblem: ApplicationProblem?,
) {
    init {
        require(configuredRelays.distinct() == configuredRelays) { "Configured relays must be unique" }
        configuredRelays.forEach { requireSafeText(it, "Configured relay", 2048) }
        require(identities.map(IdentitySummary::id).distinct().size == identities.size) {
            "Snapshot identities must be unique"
        }
        val ids = identities.map(IdentitySummary::id).toSet()
        require(selectedIdentityId == null || selectedIdentityId in ids) {
            "Selected identity is not present in the snapshot"
        }
        require(sessionSubjectIdentityId == null || sessionSubjectIdentityId in ids) {
            "Session subject is not present in the snapshot"
        }
        require(activeIdentity == null || activeIdentity.identity in identities) {
            "Active identity does not match an identity in the snapshot"
        }
        require((session == SessionLifecycle.Active) == (activeIdentity != null)) {
            "Active session and active identity must agree"
        }
        require(activeIdentity == null || activeIdentity.relays.destinations == configuredRelays) {
            "Active identity relays do not match configured relays"
        }
    }
}

private fun validateOptional(
    value: String?,
    label: String,
    maximumLength: Int,
) {
    if (value != null) requireSafeText(value, label, maximumLength)
}

private fun requireSafeText(
    value: String,
    label: String,
    maximumLength: Int,
) {
    require(value.isNotBlank() && value.length <= maximumLength && value.none(Char::isISOControl)) {
        "$label is empty, oversized, or contains a control character"
    }
}
