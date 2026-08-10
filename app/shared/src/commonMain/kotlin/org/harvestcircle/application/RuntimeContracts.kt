package org.harvestcircle.application

import kotlinx.coroutines.flow.Flow

sealed interface ApplicationCommand {
    data class AcknowledgeGeneratedIdentity(
        val requestId: RecoveryRequestId,
        val context: RequestContext,
    ) : ApplicationCommand

    data class CancelGeneratedIdentity(
        val requestId: RecoveryRequestId,
    ) : ApplicationCommand

    data class ImportLocalIdentity(
        val secretKey: SecretKeyInput,
        val context: RequestContext,
    ) : ApplicationCommand

    data class SelectIdentity(
        val identityId: IdentityId,
    ) : ApplicationCommand

    data class ActivateIdentity(
        val identityId: IdentityId,
    ) : ApplicationCommand

    data object SignOut : ApplicationCommand

    data object RefreshActiveProfile : ApplicationCommand

    data class ConfirmIdentityRemoval(
        val requestId: RemovalRequestId,
        val context: RequestContext,
    ) : ApplicationCommand
}

sealed interface ApplicationCommandResult {
    val snapshot: ApplicationSnapshot

    data class Committed(
        val operationId: OperationId,
        val committedRevision: SnapshotRevision,
        override val snapshot: ApplicationSnapshot,
    ) : ApplicationCommandResult {
        init {
            require(committedRevision == snapshot.revision) {
                "Committed revision does not match the returned snapshot"
            }
        }
    }

    data class Updated(
        override val snapshot: ApplicationSnapshot,
    ) : ApplicationCommandResult
}

data class GeneratedIdentityRecovery(
    val requestId: RecoveryRequestId,
    val identity: IdentitySummary,
    val expiresAt: UnixSeconds,
    val backup: GeneratedKeyBackup,
)

data class IdentityRemovalRequest(
    val requestId: RemovalRequestId,
    val identityId: IdentityId,
    val deletesLocalCredential: Boolean,
    val signsOut: Boolean,
    val expiresAt: UnixSeconds,
)

data class ApplicationChange(
    val snapshot: ApplicationSnapshot,
    val previousRevision: SnapshotRevision?,
) {
    init {
        require(previousRevision == null || previousRevision.value < snapshot.revision.value) {
            "Application change revisions are not monotonic"
        }
    }
}

data class ShutdownReceipt(
    val finalRevision: SnapshotRevision,
    val closed: Boolean,
)

interface HarvestCircleRuntime {
    suspend fun bootstrap(): ApplicationSnapshot

    fun currentSnapshot(): ApplicationSnapshot

    fun changes(): Flow<ApplicationChange>

    suspend fun execute(command: ApplicationCommand): ApplicationCommandResult

    suspend fun prepareLocalIdentity(): GeneratedIdentityRecovery

    suspend fun requestIdentityRemoval(identityId: IdentityId): IdentityRemovalRequest

    suspend fun shutdown(): ShutdownReceipt
}
