package org.harvestcircle.application

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class ShellStatusTest {
    @Test
    fun derivesLiveSignerAndSyncFromCurrentEvidence() {
        val active = deriveShellStatus(shellState(RelayConnectionState.Connected))
        assertEquals(SyncStatusLabel.Available, active.sync)
        assertEquals(SignerStatusLabel.Available, active.signer)
        assertNull(active.banner)

        val readOnly = deriveShellStatus(shellState(active = false, readOnly = true))
        assertEquals(SignerStatusLabel.ReadOnly, readOnly.signer)
        assertEquals("Read-only", readOnly.banner?.title)
    }

    @Test
    fun bannerPriorityPrefersLocalDataThenConnectionThenSigner() {
        val storageProblem = problem(ApplicationErrorCategory.Storage, ApplicationErrorCode.StorageUnavailable)
        val localData = deriveShellStatus(shellState(RelayConnectionState.Error, lastProblem = storageProblem))
        assertEquals("Local data needs attention", localData.banner?.title)

        val offline = deriveShellStatus(shellState(RelayConnectionState.Error))
        assertEquals("Offline", offline.banner?.title)

        val signerMissing = deriveShellStatus(shellState(signer = SignerAvailability.CredentialMissing))
        assertEquals("Signer unavailable", signerMissing.banner?.title)
    }
}

private fun shellState(
    relay: RelayConnectionState = RelayConnectionState.Connected,
    signer: SignerAvailability = SignerAvailability.Available,
    active: Boolean = true,
    readOnly: Boolean = false,
    lastProblem: ApplicationProblem? = null,
): HarvestCircleShellState {
    val identity =
        IdentitySummary(
            IdentityId.fromPublicKeyHex("03".repeat(32)),
            "npub1status",
            "Status identity",
            SignerBindingSummary(SignerBindingKind.LocalKeyring, signer),
            UnixSeconds(1),
            null,
        )
    val snapshot =
        ApplicationSnapshot(
            SnapshotRevision(1UL),
            ApplicationLifecycle.Ready,
            lifecycleProblem = null,
            configuredRelays = emptyList(),
            identities = listOf(identity),
            selectedIdentityId = identity.id,
            session = if (active) SessionLifecycle.Active else SessionLifecycle.SignedOut,
            sessionSubjectIdentityId = identity.id.takeIf { active },
            sessionProblem = null,
            activeIdentity =
                ActiveIdentity(identity, RelaySummary(emptyList(), relay), ProfileLoadState.Empty, null).takeIf { active },
            recoverableProblem = null,
        )
    return HarvestCircleShellState(
        identity = HarvestCirclePresenterState(snapshot, lastProblem = lastProblem),
        buildInfo = BuildInfo.unknown(),
        session = ShellSessionState(readOnly),
    )
}

private fun problem(
    category: ApplicationErrorCategory,
    code: ApplicationErrorCode,
) = ApplicationProblem(code, category, true, RecoveryAction.Retry, null, "Safe status problem.")
