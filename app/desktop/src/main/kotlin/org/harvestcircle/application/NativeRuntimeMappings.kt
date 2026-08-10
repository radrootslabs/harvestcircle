package org.harvestcircle.application

import org.harvestcircle.ffi.AccountCommandReceiptDto
import org.harvestcircle.ffi.AccountDto
import org.harvestcircle.ffi.ActiveAccountDto
import org.harvestcircle.ffi.AppLifecycleDto
import org.harvestcircle.ffi.AppSnapshotDto
import org.harvestcircle.ffi.HarvestCircleException
import org.harvestcircle.ffi.KeyAvailabilityDto
import org.harvestcircle.ffi.ProfileDto
import org.harvestcircle.ffi.ProfileLoadStateDto
import org.harvestcircle.ffi.RelayConnectionStateDto
import org.harvestcircle.ffi.RequestContextDto
import org.harvestcircle.ffi.SafeErrorDto
import org.harvestcircle.ffi.SessionStateDto
import org.harvestcircle.ffi.ShutdownReceiptDto
import org.harvestcircle.ffi.SignerKindDto
import org.harvestcircle.ffi.SnapshotChangeDto
import org.harvestcircle.ffi.WireErrorCategory
import org.harvestcircle.ffi.WireErrorCode
import org.harvestcircle.ffi.WireRecoveryAction

internal fun RequestContext.toNative(): RequestContextDto =
    RequestContextDto(
        requestId = operationId.value,
        expectedRevision = expectedRevision.value,
        deadlineMillis = deadlineMillis,
    )

internal fun AccountCommandReceiptDto.toApplicationResult(): ApplicationCommandResult.Committed =
    ApplicationCommandResult.Committed(
        operationId = OperationId.from(requestId),
        committedRevision = SnapshotRevision(committedRevision),
        snapshot = snapshot.toApplicationSnapshot(),
    )

internal fun AccountDto.toIdentitySummary(): IdentitySummary =
    IdentitySummary(
        id = IdentityId.fromPublicKeyHex(publicKeyHex),
        npub = npub,
        displayLabel = displayLabel,
        signer = SignerBindingSummary(signerKind.toSignerBindingKind(), keyAvailability.toSignerAvailability()),
        createdAt = UnixSeconds(createdAtSeconds),
        lastUsedAt = lastUsedAtSeconds?.let(::UnixSeconds),
    )

internal fun ActiveAccountDto.toActiveIdentity(configuredRelays: List<String>): ActiveIdentity =
    ActiveIdentity(
        identity = account.toIdentitySummary(),
        relays = RelaySummary(configuredRelays, relayState.toRelayConnectionState()),
        profileState = profileState.toProfileLoadState(),
        profile = profile?.toProfileSummary(),
    )

internal fun AppSnapshotDto.toApplicationSnapshot(): ApplicationSnapshot =
    ApplicationSnapshot(
        revision = SnapshotRevision(revision),
        lifecycle = lifecycle.toApplicationLifecycle(),
        lifecycleProblem = lifecycleError?.toApplicationProblem(),
        configuredRelays = configuredRelays,
        identities = accounts.map(AccountDto::toIdentitySummary),
        selectedIdentityId = selectedPublicKeyHex?.let(IdentityId::fromPublicKeyHex),
        session = session.toSessionLifecycle(),
        sessionSubjectIdentityId = sessionSubjectPublicKeyHex?.let(IdentityId::fromPublicKeyHex),
        sessionProblem = sessionError?.toApplicationProblem(),
        activeIdentity = activeAccount?.toActiveIdentity(configuredRelays),
        recoverableProblem = recoverableProblem?.toApplicationProblem(),
    )

internal fun ProfileDto.toProfileSummary(): ProfileSummary =
    ProfileSummary(
        name = name,
        displayName = displayName,
        nip05 = nip05,
        about = about,
        picture = picture,
    )

internal fun SafeErrorDto.toApplicationProblem(): ApplicationProblem =
    ApplicationProblem(
        code = code.toApplicationErrorCode(),
        category = category.toApplicationErrorCategory(),
        retryable = retryable,
        recoveryAction = recoveryAction.toRecoveryAction(),
        operationId = null,
        safeMessage = message,
    )

internal fun SnapshotChangeDto.toApplicationChange(): ApplicationChange =
    ApplicationChange(
        snapshot = snapshot.toApplicationSnapshot(),
        previousRevision = previousRevision?.let(::SnapshotRevision),
    )

internal fun ShutdownReceiptDto.toShutdownReceipt(): ShutdownReceipt =
    ShutdownReceipt(
        finalRevision = SnapshotRevision(finalRevision),
        closed = closed,
    )

internal fun Throwable.toApplicationProblem(fallbackOperationId: OperationId? = null): ApplicationProblem {
    val native = this as? HarvestCircleException.Failure
    return ApplicationProblem(
        code = native?.code?.toApplicationErrorCode() ?: ApplicationErrorCode.Internal,
        category = native?.category?.toApplicationErrorCategory() ?: ApplicationErrorCategory.Internal,
        retryable = native?.retryable ?: false,
        recoveryAction = native?.recoveryAction?.toRecoveryAction() ?: RecoveryAction.None,
        operationId = native?.correlationId?.let { runCatching { OperationId.from(it) }.getOrNull() } ?: fallbackOperationId,
        safeMessage = native?.safeMessage ?: "The application command failed.",
    )
}

internal fun AppLifecycleDto.toApplicationLifecycle(): ApplicationLifecycle =
    when (this) {
        AppLifecycleDto.OPENING -> ApplicationLifecycle.Opening
        AppLifecycleDto.COMPATIBILITY_CHECKING -> ApplicationLifecycle.CompatibilityChecking
        AppLifecycleDto.ACQUIRING_OWNERSHIP -> ApplicationLifecycle.AcquiringOwnership
        AppLifecycleDto.MIGRATING -> ApplicationLifecycle.Migrating
        AppLifecycleDto.RECOVERING -> ApplicationLifecycle.Recovering
        AppLifecycleDto.READY -> ApplicationLifecycle.Ready
        AppLifecycleDto.DEGRADED -> ApplicationLifecycle.Degraded
        AppLifecycleDto.BLOCKED -> ApplicationLifecycle.Blocked
        AppLifecycleDto.SHUTTING_DOWN -> ApplicationLifecycle.ShuttingDown
        AppLifecycleDto.CLOSED -> ApplicationLifecycle.Closed
        AppLifecycleDto.FATAL -> ApplicationLifecycle.Fatal
    }

internal fun SessionStateDto.toSessionLifecycle(): SessionLifecycle =
    when (this) {
        SessionStateDto.SIGNED_OUT -> SessionLifecycle.SignedOut
        SessionStateDto.ACTIVATING -> SessionLifecycle.Activating
        SessionStateDto.ACTIVE -> SessionLifecycle.Active
        SessionStateDto.SIGNING_OUT -> SessionLifecycle.SigningOut
        SessionStateDto.FAILED -> SessionLifecycle.Failed
    }

internal fun SignerKindDto.toSignerBindingKind(): SignerBindingKind =
    when (this) {
        SignerKindDto.LOCAL_SECRET -> SignerBindingKind.LocalKeyring
        SignerKindDto.WATCH_ONLY -> SignerBindingKind.Unsupported("read-only")
        SignerKindDto.REMOTE_NIP46 -> SignerBindingKind.Unsupported("remote-nip46")
    }

internal fun KeyAvailabilityDto.toSignerAvailability(): SignerAvailability =
    when (this) {
        KeyAvailabilityDto.AVAILABLE -> SignerAvailability.Available
        KeyAvailabilityDto.CREDENTIAL_MISSING -> SignerAvailability.CredentialMissing
        KeyAvailabilityDto.STORE_UNAVAILABLE -> SignerAvailability.StoreUnavailable
        KeyAvailabilityDto.NOT_REQUIRED -> SignerAvailability.NotRequired
    }

internal fun RelayConnectionStateDto.toRelayConnectionState(): RelayConnectionState =
    when (this) {
        RelayConnectionStateDto.DISCONNECTED -> RelayConnectionState.Disconnected
        RelayConnectionStateDto.CONNECTING -> RelayConnectionState.Connecting
        RelayConnectionStateDto.CONNECTED -> RelayConnectionState.Connected
        RelayConnectionStateDto.DEGRADED -> RelayConnectionState.Degraded
        RelayConnectionStateDto.ERROR -> RelayConnectionState.Error
    }

internal fun ProfileLoadStateDto.toProfileLoadState(): ProfileLoadState =
    when (this) {
        ProfileLoadStateDto.EMPTY -> ProfileLoadState.Empty
        ProfileLoadStateDto.LOADING -> ProfileLoadState.Loading
        ProfileLoadStateDto.CACHED -> ProfileLoadState.Cached
        ProfileLoadStateDto.FRESH -> ProfileLoadState.Fresh
        ProfileLoadStateDto.ERROR -> ProfileLoadState.Error
    }

internal fun WireErrorCode.toApplicationErrorCode(): ApplicationErrorCode =
    when (this) {
        WireErrorCode.INVALID_PUBLIC_KEY -> ApplicationErrorCode.InvalidPublicKey
        WireErrorCode.INVALID_SECRET_KEY -> ApplicationErrorCode.InvalidSecretKey
        WireErrorCode.INVALID_ACCOUNT_METADATA -> ApplicationErrorCode.InvalidIdentityMetadata
        WireErrorCode.INVALID_PROFILE_METADATA -> ApplicationErrorCode.InvalidProfileMetadata
        WireErrorCode.INVALID_APPLICATION_STATE -> ApplicationErrorCode.InvalidApplicationState
        WireErrorCode.ACCOUNT_ALREADY_EXISTS -> ApplicationErrorCode.IdentityAlreadyExists
        WireErrorCode.ACCOUNT_NOT_FOUND -> ApplicationErrorCode.IdentityNotFound
        WireErrorCode.KEYRING_UNAVAILABLE -> ApplicationErrorCode.KeyringUnavailable
        WireErrorCode.CREDENTIAL_MISSING -> ApplicationErrorCode.CredentialMissing
        WireErrorCode.STORAGE_UNAVAILABLE -> ApplicationErrorCode.StorageUnavailable
        WireErrorCode.STORAGE_CORRUPT -> ApplicationErrorCode.StorageCorrupt
        WireErrorCode.STORAGE_QUARANTINED -> ApplicationErrorCode.StorageQuarantined
        WireErrorCode.STORAGE_BACKUP_INVALID -> ApplicationErrorCode.StorageBackupInvalid
        WireErrorCode.UNSUPPORTED_SCHEMA_VERSION -> ApplicationErrorCode.UnsupportedSchemaVersion
        WireErrorCode.REPAIR_UNAUTHORIZED -> ApplicationErrorCode.RepairUnauthorized
        WireErrorCode.PENDING_OPERATION_RECOVERY_REQUIRED -> ApplicationErrorCode.PendingOperationRecoveryRequired
        WireErrorCode.INVALID_RELAY_CONFIGURATION -> ApplicationErrorCode.InvalidRelayConfiguration
        WireErrorCode.RELAY_CONNECTION_FAILED -> ApplicationErrorCode.RelayConnectionFailed
        WireErrorCode.PROFILE_REFRESH_FAILED -> ApplicationErrorCode.ProfileRefreshFailed
        WireErrorCode.OBSERVER_REGISTRATION_FAILED -> ApplicationErrorCode.ObserverRegistrationFailed
        WireErrorCode.NATIVE_LIBRARY_LOAD_FAILED -> ApplicationErrorCode.NativeLibraryLoadFailed
        WireErrorCode.COMPATIBILITY_MISMATCH -> ApplicationErrorCode.CompatibilityMismatch
        WireErrorCode.INTERNAL -> ApplicationErrorCode.Internal
    }

internal fun WireErrorCategory.toApplicationErrorCategory(): ApplicationErrorCategory =
    when (this) {
        WireErrorCategory.INPUT -> ApplicationErrorCategory.Input
        WireErrorCategory.CONFLICT -> ApplicationErrorCategory.Conflict
        WireErrorCategory.CREDENTIAL -> ApplicationErrorCategory.Credential
        WireErrorCategory.STORAGE -> ApplicationErrorCategory.Storage
        WireErrorCategory.NETWORK -> ApplicationErrorCategory.Network
        WireErrorCategory.LIFECYCLE -> ApplicationErrorCategory.Lifecycle
        WireErrorCategory.COMPATIBILITY -> ApplicationErrorCategory.Compatibility
        WireErrorCategory.INTERNAL -> ApplicationErrorCategory.Internal
    }

internal fun WireRecoveryAction.toRecoveryAction(): RecoveryAction =
    when (this) {
        WireRecoveryAction.NONE -> RecoveryAction.None
        WireRecoveryAction.RETRY -> RecoveryAction.Retry
        WireRecoveryAction.REPAIR_CREDENTIAL -> RecoveryAction.RepairCredential
        WireRecoveryAction.AUTHENTICATE -> RecoveryAction.Authenticate
        WireRecoveryAction.REPAIR_STORAGE -> RecoveryAction.RepairStorage
        WireRecoveryAction.RESTORE_BACKUP -> RecoveryAction.RestoreBackup
        WireRecoveryAction.CHECK_CONFIGURATION -> RecoveryAction.CheckConfiguration
        WireRecoveryAction.RESTART_APPLICATION -> RecoveryAction.RestartApplication
        WireRecoveryAction.UPDATE_APPLICATION -> RecoveryAction.UpdateApplication
    }
