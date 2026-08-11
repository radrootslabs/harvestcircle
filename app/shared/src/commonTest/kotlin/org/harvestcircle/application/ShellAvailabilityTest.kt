package org.harvestcircle.application

import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.BootstrapStep
import org.harvestcircle.navigation.NavigationState
import org.harvestcircle.product.ScreenKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertSame
import kotlin.test.assertTrue

class ShellAvailabilityTest {
    @Test
    fun nativeRootAndSessionDeriveTruthfulFoundationRoutes() {
        val signedOut = presenterState(HarvestCircleRoute.IDENTITIES)
        assertEquals(ShellRoot.BootstrapCanvas(BootstrapStep.Welcome), deriveShellRoot(signedOut, ShellSessionState()))
        val readOnly = deriveShellRoot(signedOut, ShellSessionState().enterReadOnly()) as ShellRoot.Dashboard
        assertEquals(AppRoute.PersonalToday, readOnly.navigation.current)
        val active = deriveShellRoot(presenterState(readyActiveSnapshot()), ShellSessionState())
        assertTrue(active is ShellRoot.Dashboard)
        assertTrue(deriveShellRoot(presenterState(HarvestCircleRoute.FATAL), ShellSessionState()) is ShellRoot.LifecycleCanvas)
    }

    @Test
    fun readOnlyIsSessionLocalAndRestartDefaultsToBootstrap() {
        assertTrue(ShellSessionState().enterReadOnly().readOnly)
        assertFalse(ShellSessionState().readOnly)
        val restarted = deriveShellRoot(presenterState(HarvestCircleRoute.IDENTITIES), ShellSessionState())
        assertTrue(restarted is ShellRoot.BootstrapCanvas)
    }

    @Test
    fun bootstrapEntryAndRecoveryFollowPresenterTruth() {
        val signedOut = presenterState(HarvestCircleRoute.IDENTITIES)
        assertEquals(
            ShellRoot.BootstrapCanvas(BootstrapStep.CreateIdentity),
            deriveShellRoot(signedOut.copy(identityEntryMode = IdentityEntryMode.CREATE), ShellSessionState()),
        )
        assertEquals(
            ShellRoot.BootstrapCanvas(BootstrapStep.ImportIdentity),
            deriveShellRoot(signedOut.copy(identityEntryMode = IdentityEntryMode.IMPORT), ShellSessionState()),
        )
        assertEquals(
            ShellRoot.BootstrapCanvas(BootstrapStep.GeneratedRecovery),
            deriveShellRoot(
                signedOut.copy(generatedKeyBackup = GeneratedKeyBackup("npub1generated", "nsec1generated")),
                ShellSessionState(),
            ),
        )
    }

    @Test
    fun typedDegradationAllowsOnlyNetworkAndCredentialProblems() {
        assertEquals(
            LocalUsability.UsableDegraded(DegradationReason.Network),
            deriveLocalUsability(degradedSnapshot(problem(ApplicationErrorCategory.Network))),
        )
        assertEquals(
            LocalUsability.UsableDegraded(DegradationReason.Credential),
            deriveLocalUsability(degradedSnapshot(problem(ApplicationErrorCategory.Credential))),
        )
        assertEquals(
            LocalUsability.Unusable,
            deriveLocalUsability(degradedSnapshot(problem(ApplicationErrorCategory.Storage))),
        )
        assertEquals(LocalUsability.Unusable, deriveLocalUsability(degradedSnapshot(null)))
    }

    @Test
    fun usableDegradedSnapshotsStayInTheProductShell() {
        val networkProblem = problem(ApplicationErrorCategory.Network)
        val empty = presenterState(degradedSnapshot(networkProblem))
        assertEquals(ShellRoot.BootstrapCanvas(BootstrapStep.Welcome), deriveShellRoot(empty, ShellSessionState()))

        val savedIdentity = identity()
        val signedOut = presenterState(degradedSnapshot(networkProblem, identities = listOf(savedIdentity)))
        assertEquals(
            ShellRoot.BootstrapCanvas(BootstrapStep.IdentityChooser),
            deriveShellRoot(signedOut, ShellSessionState()),
        )

        val readOnly = deriveShellRoot(empty, ShellSessionState().enterReadOnly())
        assertTrue(readOnly is ShellRoot.Dashboard)

        val active = presenterState(degradedSnapshot(networkProblem, activeIdentity = activeIdentity(savedIdentity)))
        assertTrue(deriveShellRoot(active, ShellSessionState()) is ShellRoot.Dashboard)
    }

    @Test
    fun unusableDegradationFailsClosedWithoutInspectingMessageText() {
        val storage = presenterState(degradedSnapshot(problem(ApplicationErrorCategory.Storage, "network unavailable")))
        val compatibility =
            presenterState(degradedSnapshot(problem(ApplicationErrorCategory.Compatibility, "credential unavailable")))
        assertTrue(deriveShellRoot(storage, ShellSessionState()) is ShellRoot.LifecycleCanvas)
        assertTrue(deriveShellRoot(compatibility, ShellSessionState()) is ShellRoot.LifecycleCanvas)
    }

    @Test
    fun disabledFeaturesCannotDispatch() {
        val state = NavigationState(AppRoute.PersonalToday)
        assertSame(state, activateShellScreen(state, ScreenKey.Explore))
        assertSame(state, activateShellScreen(state, ScreenKey.Activity))
        assertSame(state, activateShellScreen(state, ScreenKey.FarmOverview))
        assertEquals(AppRoute.Network, activateShellScreen(state, ScreenKey.Network).current)
        assertTrue(shellNavigationItems.filterNot(ShellNavigationItem::enabled).all { it.route == null })
        assertFalse(addFarmWorkspaceAction.enabled)
    }
}

private fun presenterState(route: HarvestCircleRoute): HarvestCirclePresenterState =
    HarvestCirclePresenterState(
        snapshot =
            ApplicationSnapshot(
                revision = SnapshotRevision(1UL),
                lifecycle = ApplicationLifecycle.Ready,
                lifecycleProblem = null,
                configuredRelays = emptyList(),
                identities = emptyList(),
                selectedIdentityId = null,
                session = SessionLifecycle.SignedOut,
                sessionSubjectIdentityId = null,
                sessionProblem = null,
                activeIdentity = null,
                recoverableProblem = null,
            ),
        route = route,
    )

private fun presenterState(snapshot: ApplicationSnapshot): HarvestCirclePresenterState = HarvestCirclePresenterState(snapshot)

private fun degradedSnapshot(
    problem: ApplicationProblem?,
    activeIdentity: ActiveIdentity? = null,
    identities: List<IdentitySummary> = activeIdentity?.identity?.let { listOf(it) } ?: emptyList(),
): ApplicationSnapshot =
    ApplicationSnapshot(
        revision = SnapshotRevision(2UL),
        lifecycle = ApplicationLifecycle.Degraded,
        lifecycleProblem = problem,
        configuredRelays = emptyList(),
        identities = identities,
        selectedIdentityId = activeIdentity?.identity?.id,
        session = if (activeIdentity == null) SessionLifecycle.SignedOut else SessionLifecycle.Active,
        sessionSubjectIdentityId = activeIdentity?.identity?.id,
        sessionProblem = null,
        activeIdentity = activeIdentity,
        recoverableProblem = null,
    )

private fun readyActiveSnapshot(): ApplicationSnapshot {
    val identity = identity()
    return ApplicationSnapshot(
        revision = SnapshotRevision(1UL),
        lifecycle = ApplicationLifecycle.Ready,
        lifecycleProblem = null,
        configuredRelays = emptyList(),
        identities = listOf(identity),
        selectedIdentityId = identity.id,
        session = SessionLifecycle.Active,
        sessionSubjectIdentityId = identity.id,
        sessionProblem = null,
        activeIdentity = activeIdentity(identity),
        recoverableProblem = null,
    )
}

private fun problem(
    category: ApplicationErrorCategory,
    message: String = "The application is degraded.",
): ApplicationProblem =
    ApplicationProblem(
        code =
            when (category) {
                ApplicationErrorCategory.Network -> ApplicationErrorCode.RelayConnectionFailed
                ApplicationErrorCategory.Credential -> ApplicationErrorCode.CredentialMissing
                ApplicationErrorCategory.Storage -> ApplicationErrorCode.StorageUnavailable
                ApplicationErrorCategory.Compatibility -> ApplicationErrorCode.CompatibilityMismatch
                else -> ApplicationErrorCode.Internal
            },
        category = category,
        retryable = true,
        recoveryAction = RecoveryAction.Retry,
        operationId = null,
        safeMessage = message,
    )

private fun identity(): IdentitySummary =
    IdentitySummary(
        id = IdentityId.fromPublicKeyHex("01".repeat(32)),
        npub = "npub1degraded",
        displayLabel = "Degraded identity",
        signer = SignerBindingSummary(SignerBindingKind.LocalKeyring, SignerAvailability.Available),
        createdAt = UnixSeconds(1),
        lastUsedAt = null,
    )

private fun activeIdentity(identity: IdentitySummary): ActiveIdentity =
    ActiveIdentity(
        identity = identity,
        relays = RelaySummary(emptyList(), RelayConnectionState.Degraded),
        profileState = ProfileLoadState.Cached,
        profile = null,
    )
