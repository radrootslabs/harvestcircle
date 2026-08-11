package org.harvestcircle.ui.shell

import org.harvestcircle.application.ActiveIdentity
import org.harvestcircle.application.ApplicationErrorCategory
import org.harvestcircle.application.ApplicationErrorCode
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.application.ApplicationProblem
import org.harvestcircle.application.ApplicationSnapshot
import org.harvestcircle.application.BuildInfo
import org.harvestcircle.application.HarvestCirclePresenterState
import org.harvestcircle.application.HarvestCircleRoute
import org.harvestcircle.application.HarvestCircleShellState
import org.harvestcircle.application.IdentityId
import org.harvestcircle.application.IdentitySummary
import org.harvestcircle.application.ProfileLoadState
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.RelayConnectionState
import org.harvestcircle.application.RelayDestination
import org.harvestcircle.application.RelayEndpoint
import org.harvestcircle.application.RelaySummary
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.ShellSessionState
import org.harvestcircle.application.SignerAvailability
import org.harvestcircle.application.SignerBindingKind
import org.harvestcircle.application.SignerBindingSummary
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.application.UnixSeconds
import kotlin.test.Test
import kotlin.test.assertEquals

class FoundationNetworkModelTest {
    @Test
    fun mapsActiveRelayRuntimeAndOperationTruth() {
        val model = foundationNetworkModel(shellState())
        assertEquals("Local identity active", model.identityState)
        assertEquals("Grower identity", model.identityLabel)
        assertEquals("Degraded", model.relayState)
        assertEquals("Public", model.relays.single().destination)
        assertEquals("Read available", model.relays.single().readState)
        assertEquals("Write unavailable", model.relays.single().writeState)
        assertEquals("Degraded", model.runtimeState)
        assertEquals("Runtime degraded.", model.runtimeProblem)
        assertEquals(1, model.pendingOperations)
    }

    @Test
    fun readOnlyOverridesSignerAuthorityWithoutInventingNetworkState() {
        val model = foundationNetworkModel(shellState(readOnly = true, active = false))
        assertEquals("Read-only", model.identityState)
        assertEquals("Not yet observed", model.relayState)
    }
}

private fun shellState(
    readOnly: Boolean = false,
    active: Boolean = true,
): HarvestCircleShellState {
    val identity =
        IdentitySummary(
            IdentityId.fromPublicKeyHex("11".repeat(32)),
            "npub1grower",
            "Grower identity",
            SignerBindingSummary(SignerBindingKind.LocalKeyring, SignerAvailability.Available),
            UnixSeconds(1),
            null,
        )
    val relay = RelayEndpoint("wss://relay.example", RelayDestination.Public, read = true, write = false)
    val problem =
        ApplicationProblem(
            ApplicationErrorCode.StorageUnavailable,
            ApplicationErrorCategory.Storage,
            retryable = true,
            RecoveryAction.Retry,
            operationId = null,
            safeMessage = "Runtime degraded.",
        )
    val snapshot =
        ApplicationSnapshot(
            SnapshotRevision(2UL),
            ApplicationLifecycle.Degraded,
            lifecycleProblem = problem,
            configuredRelays = listOf(relay),
            identities = listOf(identity),
            selectedIdentityId = identity.id,
            session = if (active) SessionLifecycle.Active else SessionLifecycle.SignedOut,
            sessionSubjectIdentityId = identity.id.takeIf { active },
            sessionProblem = null,
            activeIdentity =
                ActiveIdentity(
                    identity,
                    RelaySummary(listOf(relay), RelayConnectionState.Degraded),
                    ProfileLoadState.Empty,
                    profile = null,
                ).takeIf { active },
            recoverableProblem = null,
        )
    val presenter = HarvestCirclePresenterState(snapshot, route = HarvestCircleRoute.ACTIVE_IDENTITY, busy = true)
    return HarvestCircleShellState(
        identity = presenter,
        buildInfo = BuildInfo.unknown(),
        session = ShellSessionState(readOnly),
    )
}
