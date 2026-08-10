package org.harvestcircle.integration

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.isActive
import org.harvestcircle.application.ActiveIdentity
import org.harvestcircle.application.ApplicationChange
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.ApplicationCommandResult
import org.harvestcircle.application.ApplicationErrorCategory
import org.harvestcircle.application.ApplicationErrorCode
import org.harvestcircle.application.ApplicationFailure
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.application.ApplicationProblem
import org.harvestcircle.application.ApplicationSnapshot
import org.harvestcircle.application.BuildInfo
import org.harvestcircle.application.GeneratedIdentityRecovery
import org.harvestcircle.application.GeneratedKeyBackup
import org.harvestcircle.application.HarvestCircleRuntime
import org.harvestcircle.application.IdentityId
import org.harvestcircle.application.IdentityRemovalRequest
import org.harvestcircle.application.IdentitySummary
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.ProfileLoadState
import org.harvestcircle.application.ProfileSummary
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.RecoveryRequestId
import org.harvestcircle.application.RelayConnectionState
import org.harvestcircle.application.RelaySummary
import org.harvestcircle.application.RemovalRequestId
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.ShutdownReceipt
import org.harvestcircle.application.SignerAvailability
import org.harvestcircle.application.SignerBindingKind
import org.harvestcircle.application.SignerBindingSummary
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.application.UnixSeconds
import org.harvestcircle.testbridge.ffi.HarvestCircleTestBridge
import org.harvestcircle.testbridge.ffi.TestBridgeException
import org.harvestcircle.testbridge.ffi.TestIdentity
import org.harvestcircle.testbridge.ffi.TestSnapshot

internal class TestBridgeHarvestCircleRuntime private constructor(
    private val bridge: HarvestCircleTestBridge,
) : HarvestCircleRuntime,
    AutoCloseable {
    override val buildInfo: BuildInfo = BuildInfo.unknown()

    private var generatedRequest: RecoveryRequestId? = null
    private var closed = false
    private var shutdownReceipt: ShutdownReceipt? = null

    override suspend fun bootstrap(): ApplicationSnapshot = callBridge { bridge.bootstrap().toApplicationSnapshot() }

    override fun currentSnapshot(): ApplicationSnapshot = callBridge { bridge.snapshot().toApplicationSnapshot() }

    override fun changes(): Flow<ApplicationChange> =
        flow {
            callBridge { bridge.startObserver() }
            var previous: SnapshotRevision? = null
            try {
                while (currentCoroutineContext().isActive) {
                    val snapshot = callBridge { bridge.nextObservedSnapshot(250UL) } ?: continue
                    val mapped = snapshot.toApplicationSnapshot()
                    emit(ApplicationChange(mapped, previous))
                    previous = mapped.revision
                }
            } finally {
                runCatching { bridge.stopObserver() }
            }
        }.flowOn(Dispatchers.IO)

    override suspend fun execute(command: ApplicationCommand): ApplicationCommandResult =
        when (command) {
            is ApplicationCommand.AcknowledgeGeneratedIdentity -> {
                require(command.requestId == generatedRequest) { "Generated recovery request does not match" }
                generatedRequest = null
                val snapshot =
                    callBridge {
                        bridge
                            .acknowledgeGeneratedIdentity(
                                command.context.operationId.value,
                                command.context.expectedRevision.value,
                                command.context.deadlineMillis,
                            ).toApplicationSnapshot()
                    }
                ApplicationCommandResult.Committed(command.context.operationId, snapshot.revision, snapshot)
            }

            is ApplicationCommand.CancelGeneratedIdentity -> {
                require(command.requestId == generatedRequest) { "Generated recovery request does not match" }
                generatedRequest = null
                callBridge { bridge.cancelGeneratedIdentity() }
                ApplicationCommandResult.Updated(currentSnapshot())
            }

            is ApplicationCommand.ImportLocalIdentity -> {
                val secret = command.secretKey.take().encodeToByteArray()
                try {
                    val snapshot =
                        callBridge {
                            bridge
                                .importIdentity(
                                    command.context.operationId.value,
                                    command.context.expectedRevision.value,
                                    secret,
                                    command.context.deadlineMillis,
                                ).toApplicationSnapshot()
                        }
                    ApplicationCommandResult.Committed(command.context.operationId, snapshot.revision, snapshot)
                } finally {
                    secret.fill(0)
                    command.secretKey.clear()
                }
            }

            is ApplicationCommand.SelectIdentity ->
                ApplicationCommandResult.Updated(callBridge { bridge.selectIdentity(command.identityId.value).toApplicationSnapshot() })

            is ApplicationCommand.ActivateIdentity ->
                ApplicationCommandResult.Updated(callBridge { bridge.activateIdentity(command.identityId.value).toApplicationSnapshot() })

            ApplicationCommand.SignOut -> ApplicationCommandResult.Updated(callBridge { bridge.signOut().toApplicationSnapshot() })
            ApplicationCommand.RefreshActiveProfile ->
                ApplicationCommandResult.Updated(callBridge { bridge.refreshActiveProfile().toApplicationSnapshot() })

            is ApplicationCommand.ConfirmIdentityRemoval -> throw unsupportedRemoval(command.context.operationId)
        }

    override suspend fun prepareLocalIdentity(): GeneratedIdentityRecovery {
        val generated = callBridge { bridge.beginGeneratedIdentity() }
        val requestId = RecoveryRequestId.from("bridge:${generated.stageId}")
        generatedRequest = requestId
        return GeneratedIdentityRecovery(
            requestId = requestId,
            identity = generated.identity.toIdentitySummary(),
            expiresAt = UnixSeconds(generated.expiresAtSeconds),
            backup = GeneratedKeyBackup(generated.identity.npub, generated.recoveryNsec),
        )
    }

    override suspend fun requestIdentityRemoval(identityId: IdentityId): IdentityRemovalRequest = throw unsupportedRemoval()

    override suspend fun cancelIdentityRemoval(requestId: RemovalRequestId): Boolean = false

    override suspend fun shutdown(): ShutdownReceipt {
        shutdownReceipt?.let { return it }
        val snapshot = callBridge { bridge.shutdown().toApplicationSnapshot() }
        closed = true
        return ShutdownReceipt(snapshot.revision, snapshot.lifecycle == ApplicationLifecycle.Closed).also {
            shutdownReceipt = it
        }
    }

    fun seedProfile(
        secret: String,
        displayName: String,
    ) = callBridge { bridge.seedProfile(secret, displayName) }

    fun restart(): ApplicationSnapshot = callBridge { bridge.restart().toApplicationSnapshot() }

    fun nativeBridge(): HarvestCircleTestBridge = bridge

    override fun close() {
        if (!closed) runCatching { bridge.shutdown() }
        closed = true
        bridge.close()
    }

    private fun unsupportedRemoval(operationId: OperationId? = null): ApplicationFailure =
        ApplicationFailure(
            ApplicationProblem(
                code = ApplicationErrorCode.InvalidApplicationState,
                category = ApplicationErrorCategory.Lifecycle,
                retryable = false,
                recoveryAction = RecoveryAction.None,
                operationId = operationId,
                safeMessage = "Identity removal is outside the integration bridge contract.",
            ),
        )

    companion object {
        fun open(dataDirectory: String): TestBridgeHarvestCircleRuntime =
            TestBridgeHarvestCircleRuntime(HarvestCircleTestBridge.open(dataDirectory))
    }
}

private fun TestIdentity.toIdentitySummary(): IdentitySummary =
    IdentitySummary(
        id = IdentityId.fromPublicKeyHex(publicKeyHex),
        npub = npub,
        displayLabel = displayLabel,
        signer = SignerBindingSummary(SignerBindingKind.LocalKeyring, SignerAvailability.Available),
        createdAt = UnixSeconds(1_700_000_000),
        lastUsedAt = null,
    )

private fun TestSnapshot.toApplicationSnapshot(): ApplicationSnapshot {
    val mappedIdentities = identities.map(TestIdentity::toIdentitySummary)
    val selected = selectedPublicKeyHex?.let(IdentityId::fromPublicKeyHex)
    val mappedSession =
        when (session) {
            "signed_out" -> SessionLifecycle.SignedOut
            "activating" -> SessionLifecycle.Activating
            "active" -> SessionLifecycle.Active
            "signing_out" -> SessionLifecycle.SigningOut
            else -> SessionLifecycle.Failed
        }
    val activeIdentity =
        if (mappedSession == SessionLifecycle.Active) {
            val identity = mappedIdentities.single { it.id == selected }
            ActiveIdentity(
                identity = identity,
                relays = RelaySummary(emptyList(), RelayConnectionState.Connected),
                profileState = if (profileDisplayName == null) ProfileLoadState.Empty else ProfileLoadState.Fresh,
                profile = profileDisplayName?.let { ProfileSummary(null, it, null, null, null) },
            )
        } else {
            null
        }
    return ApplicationSnapshot(
        revision = SnapshotRevision(revision),
        lifecycle =
            when (lifecycle) {
                "ready" -> ApplicationLifecycle.Ready
                "closed" -> ApplicationLifecycle.Closed
                "fatal" -> ApplicationLifecycle.Fatal
                else -> ApplicationLifecycle.Opening
            },
        lifecycleProblem = null,
        configuredRelays = emptyList(),
        identities = mappedIdentities,
        selectedIdentityId = selected,
        session = mappedSession,
        sessionSubjectIdentityId = if (mappedSession == SessionLifecycle.Active) selected else null,
        sessionProblem = null,
        activeIdentity = activeIdentity,
        recoverableProblem = null,
    )
}

private inline fun <T> callBridge(operation: () -> T): T =
    try {
        operation()
    } catch (error: TestBridgeException.Failure) {
        throw ApplicationFailure(
            ApplicationProblem(
                code = ApplicationErrorCode.Internal,
                category = ApplicationErrorCategory.Internal,
                retryable = false,
                recoveryAction = RecoveryAction.None,
                operationId = null,
                safeMessage = error.safeMessage,
            ),
        )
    }
