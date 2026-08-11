package org.harvestcircle.application

import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.BootstrapStep
import org.harvestcircle.navigation.NavigationState
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
        val active = deriveShellRoot(presenterState(HarvestCircleRoute.ACTIVE_IDENTITY), ShellSessionState())
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
    fun disabledFeaturesCannotDispatch() {
        val state = NavigationState(AppRoute.PersonalToday)
        assertSame(state, activateShellDestination(state, ShellDestination.Explore))
        assertSame(state, activateShellDestination(state, ShellDestination.Activity))
        assertSame(state, activateShellDestination(state, ShellDestination.AddFarm))
        assertEquals(AppRoute.Network, activateShellDestination(state, ShellDestination.Network).current)
        assertTrue(shellNavigationItems.filterNot(ShellNavigationItem::enabled).all { it.route == null })
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
