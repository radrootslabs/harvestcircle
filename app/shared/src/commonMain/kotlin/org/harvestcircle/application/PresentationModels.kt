package org.harvestcircle.application

enum class HarvestCircleRoute {
    OPENING,
    CHECKING_COMPATIBILITY,
    ACQUIRING_OWNERSHIP,
    MIGRATING,
    RECOVERING,
    IDENTITIES,
    ACTIVE_IDENTITY,
    DEGRADED,
    BLOCKED,
    SHUTTING_DOWN,
    FATAL,
    CLOSED,
}

enum class CommandStatus {
    IDLE,
    RUNNING,
    ACCEPTED,
    REJECTED_BUSY,
    REJECTED_CLOSED,
    FAILED_RETRYABLE,
    FAILED_TERMINAL,
}

enum class IdentityEntryMode {
    CHOICE,
    CREATE,
    IMPORT,
}

enum class RemovalStatus {
    NONE,
    AWAITING_CONFIRMATION,
    CONFIRMING,
    COMPLETED,
    FAILED,
}

data class RemovalImpactState(
    val identityId: IdentityId,
    val deletesLocalCredential: Boolean,
    val signsOut: Boolean,
    val expiresAt: UnixSeconds,
)

data class HarvestCirclePresenterState(
    val snapshot: ApplicationSnapshot,
    val route: HarvestCircleRoute = snapshot.toHarvestCircleRoute(),
    val importDraft: String = "",
    val generatedKeyBackup: GeneratedKeyBackup? = null,
    val pendingRemovalIdentityId: IdentityId? = null,
    val removalImpact: RemovalImpactState? = null,
    val removalStatus: RemovalStatus = RemovalStatus.NONE,
    val lastRemovedIdentityId: IdentityId? = null,
    val identityChooserVisible: Boolean = false,
    val identityEntryMode: IdentityEntryMode = IdentityEntryMode.CHOICE,
    val busy: Boolean = false,
    val commandStatus: CommandStatus = CommandStatus.IDLE,
    val lastCommandOperationId: OperationId? = null,
    val lastProblem: ApplicationProblem? = null,
    val problem: String? = null,
)

sealed interface HarvestCircleIntent {
    data class EditImportDraft(
        val value: String,
    ) : HarvestCircleIntent

    data object ChooseCreateIdentity : HarvestCircleIntent

    data object ChooseImportIdentity : HarvestCircleIntent

    data object CancelIdentityEntry : HarvestCircleIntent

    data object GenerateIdentity : HarvestCircleIntent

    data object AcknowledgeGeneratedRecovery : HarvestCircleIntent

    data object CancelGeneratedRecovery : HarvestCircleIntent

    data object ImportIdentity : HarvestCircleIntent

    data class SelectIdentity(
        val identityId: IdentityId,
    ) : HarvestCircleIntent

    data class ActivateIdentity(
        val identityId: IdentityId,
    ) : HarvestCircleIntent

    data object SignOut : HarvestCircleIntent

    data object ShowIdentityChooser : HarvestCircleIntent

    data object HideIdentityChooser : HarvestCircleIntent

    data object RefreshActiveProfile : HarvestCircleIntent

    data object RetryLastCommand : HarvestCircleIntent

    data class RequestIdentityRemoval(
        val identityId: IdentityId,
    ) : HarvestCircleIntent

    data object CancelIdentityRemoval : HarvestCircleIntent

    data object ConfirmIdentityRemoval : HarvestCircleIntent

    data object DismissProblem : HarvestCircleIntent
}

sealed interface HarvestCircleEffect {
    data class Problem(
        val problem: ApplicationProblem,
    ) : HarvestCircleEffect

    data class IdentityRemoved(
        val identityId: IdentityId,
    ) : HarvestCircleEffect
}

const val MAX_IMPORT_SECRET_CHARS: Int = 128

internal fun ApplicationSnapshot.toHarvestCircleRoute(): HarvestCircleRoute =
    when (lifecycle) {
        ApplicationLifecycle.Opening -> HarvestCircleRoute.OPENING
        ApplicationLifecycle.CompatibilityChecking -> HarvestCircleRoute.CHECKING_COMPATIBILITY
        ApplicationLifecycle.AcquiringOwnership -> HarvestCircleRoute.ACQUIRING_OWNERSHIP
        ApplicationLifecycle.Migrating -> HarvestCircleRoute.MIGRATING
        ApplicationLifecycle.Recovering -> HarvestCircleRoute.RECOVERING
        ApplicationLifecycle.Ready -> if (activeIdentity == null) HarvestCircleRoute.IDENTITIES else HarvestCircleRoute.ACTIVE_IDENTITY
        ApplicationLifecycle.Degraded -> HarvestCircleRoute.DEGRADED
        ApplicationLifecycle.Blocked -> HarvestCircleRoute.BLOCKED
        ApplicationLifecycle.ShuttingDown -> HarvestCircleRoute.SHUTTING_DOWN
        ApplicationLifecycle.Closed -> HarvestCircleRoute.CLOSED
        ApplicationLifecycle.Fatal -> HarvestCircleRoute.FATAL
    }
