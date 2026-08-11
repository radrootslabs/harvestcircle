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
import org.harvestcircle.testbridge.ffi.TestGeneratedRecoveryRequest
import org.harvestcircle.testbridge.ffi.TestIdentity
import org.harvestcircle.testbridge.ffi.TestLifecycle
import org.harvestcircle.testbridge.ffi.TestRemovalRequest
import org.harvestcircle.testbridge.ffi.TestSession
import org.harvestcircle.testbridge.ffi.TestSnapshot

internal class TestBridgeHarvestCircleRuntime private constructor(
    private val bridge: HarvestCircleTestBridge,
) : HarvestCircleRuntime,
    AutoCloseable {
    override val buildInfo: BuildInfo = BuildInfo.unknown()

    private var generatedRequest: PendingGeneratedRequest? = null
    private var removalRequest: PendingRemovalRequest? = null
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
                val pending = takeGeneratedRequest(command.requestId)
                try {
                    val snapshot =
                        callBridge {
                            bridge
                                .acknowledgeGeneratedIdentity(
                                    command.context.operationId.value,
                                    command.context.expectedRevision.value,
                                    command.context.deadlineMillis,
                                    pending.native,
                                ).toApplicationSnapshot()
                        }
                    ApplicationCommandResult.Committed(command.context.operationId, snapshot.revision, snapshot)
                } finally {
                    pending.close()
                }
            }

            is ApplicationCommand.CancelGeneratedIdentity -> {
                val pending = takeGeneratedRequest(command.requestId)
                try {
                    callBridge { check(bridge.cancelGeneratedIdentity(pending.native)) }
                    ApplicationCommandResult.Updated(currentSnapshot())
                } finally {
                    pending.close()
                }
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

            is ApplicationCommand.ConfirmIdentityRemoval -> {
                val pending = takeRemovalRequest(command.requestId)
                try {
                    val snapshot =
                        callBridge {
                            bridge
                                .confirmIdentityRemoval(
                                    command.context.operationId.value,
                                    command.context.expectedRevision.value,
                                    command.context.deadlineMillis,
                                    pending.native,
                                ).toApplicationSnapshot()
                        }
                    ApplicationCommandResult.Committed(command.context.operationId, snapshot.revision, snapshot)
                } finally {
                    pending.close()
                }
            }
        }

    override suspend fun prepareLocalIdentity(): GeneratedIdentityRecovery {
        val native = callBridge { bridge.beginGeneratedIdentity() }
        var backup: GeneratedKeyBackup? = null
        return try {
            val identity = callBridge { native.identity() }.toIdentitySummary()
            val createdBackup = GeneratedKeyBackup(identity.npub, callBridge { native.takeRecoveryNsec() })
            backup = createdBackup
            val requestId = RecoveryRequestId.from("bridge:${identity.id.value}")
            generatedRequest = PendingGeneratedRequest(requestId, native, createdBackup)
            GeneratedIdentityRecovery(
                requestId = requestId,
                identity = identity,
                expiresAt = UnixSeconds(callBridge { native.expiresAtSeconds() }),
                backup = createdBackup,
            )
        } catch (error: Exception) {
            backup?.clear()
            native.close()
            throw error
        }
    }

    override suspend fun requestIdentityRemoval(identityId: IdentityId): IdentityRemovalRequest {
        removalRequest?.cancel(bridge)
        val native = callBridge { bridge.requestIdentityRemoval(identityId.value) }
        val requestId = RemovalRequestId.from("bridge-removal:${native.publicKeyHex()}:${native.expiresAtSeconds()}")
        removalRequest = PendingRemovalRequest(requestId, native)
        return IdentityRemovalRequest(
            requestId = requestId,
            identityId = IdentityId.fromPublicKeyHex(native.publicKeyHex()),
            deletesLocalCredential = native.deletesLocalCredential(),
            signsOut = native.signsOut(),
            expiresAt = UnixSeconds(native.expiresAtSeconds()),
        )
    }

    override suspend fun cancelIdentityRemoval(requestId: RemovalRequestId): Boolean {
        val pending = removalRequest?.takeIf { it.requestId == requestId } ?: return false
        removalRequest = null
        return pending.cancel(bridge)
    }

    override suspend fun shutdown(): ShutdownReceipt {
        shutdownReceipt?.let { return it }
        generatedRequest?.close()
        generatedRequest = null
        removalRequest?.cancel(bridge)
        removalRequest = null
        val snapshot = callBridge { bridge.shutdown().toApplicationSnapshot() }
        closed = true
        return ShutdownReceipt(snapshot.revision, snapshot.lifecycle == ApplicationLifecycle.Closed).also {
            shutdownReceipt = it
        }
    }

    fun seedSelectedProfile(displayName: String) = callBridge { bridge.seedSelectedProfile(displayName) }

    fun setNetworkDegraded(degraded: Boolean) = bridge.setNetworkDegraded(degraded)

    fun restart(): ApplicationSnapshot {
        generatedRequest?.close()
        generatedRequest = null
        removalRequest?.cancel(bridge)
        removalRequest = null
        val snapshot = callBridge { bridge.restart().toApplicationSnapshot() }
        closed = false
        shutdownReceipt = null
        return snapshot
    }

    fun nativeBridge(): HarvestCircleTestBridge = bridge

    override fun close() {
        generatedRequest?.close()
        generatedRequest = null
        removalRequest?.cancel(bridge)
        removalRequest = null
        if (!closed) runCatching { bridge.shutdown() }
        closed = true
        bridge.close()
    }

    private fun takeGeneratedRequest(requestId: RecoveryRequestId): PendingGeneratedRequest {
        val pending = generatedRequest
        require(pending?.requestId == requestId) { "Generated recovery request does not match" }
        generatedRequest = null
        return pending
    }

    private fun takeRemovalRequest(requestId: RemovalRequestId): PendingRemovalRequest {
        val pending = removalRequest
        require(pending?.requestId == requestId) { "Identity removal request does not match" }
        removalRequest = null
        return pending
    }

    companion object {
        fun open(dataDirectory: String): TestBridgeHarvestCircleRuntime =
            TestBridgeHarvestCircleRuntime(HarvestCircleTestBridge.open(dataDirectory))
    }
}

private class PendingRemovalRequest(
    val requestId: RemovalRequestId,
    val native: TestRemovalRequest,
) {
    fun cancel(bridge: HarvestCircleTestBridge): Boolean =
        try {
            callBridge { bridge.cancelIdentityRemoval(native) }
        } finally {
            close()
        }

    fun close() = native.close()
}

private class PendingGeneratedRequest(
    val requestId: RecoveryRequestId,
    val native: TestGeneratedRecoveryRequest,
    private val backup: GeneratedKeyBackup,
) {
    fun close() {
        backup.clear()
        native.close()
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
            TestSession.SIGNED_OUT -> SessionLifecycle.SignedOut
            TestSession.ACTIVATING -> SessionLifecycle.Activating
            TestSession.ACTIVE -> SessionLifecycle.Active
            TestSession.SIGNING_OUT -> SessionLifecycle.SigningOut
            TestSession.FAILED -> SessionLifecycle.Failed
        }
    val activeIdentity =
        if (mappedSession == SessionLifecycle.Active) {
            val identity = mappedIdentities.single { it.id == selected }
            ActiveIdentity(
                identity = identity,
                relays =
                    RelaySummary(
                        emptyList(),
                        if (lifecycle == TestLifecycle.DEGRADED) {
                            RelayConnectionState.Degraded
                        } else {
                            RelayConnectionState.Connected
                        },
                    ),
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
                TestLifecycle.BOOTING -> ApplicationLifecycle.Opening
                TestLifecycle.READY -> ApplicationLifecycle.Ready
                TestLifecycle.DEGRADED -> ApplicationLifecycle.Degraded
                TestLifecycle.FATAL -> ApplicationLifecycle.Fatal
                TestLifecycle.CLOSED -> ApplicationLifecycle.Closed
            },
        lifecycleProblem =
            if (lifecycle == TestLifecycle.DEGRADED) {
                ApplicationProblem(
                    code = ApplicationErrorCode.RelayConnectionFailed,
                    category = ApplicationErrorCategory.Network,
                    retryable = true,
                    recoveryAction = RecoveryAction.Retry,
                    operationId = null,
                    safeMessage = "The local integration relay is unavailable.",
                )
            } else {
                null
            },
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
