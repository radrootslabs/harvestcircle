package org.harvestcircle.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.navigation.AppRoute
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class HarvestCircleShellPresenterTest {
    @Test
    fun shellComposesRootNavigationAppearanceAndOverlays() =
        runTest {
            val identity = FakeIdentityPresentation(presenterState(HarvestCircleRoute.IDENTITIES))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()
            assertTrue(presenter.state.value.root is ShellRoot.BootstrapCanvas)

            presenter.dispatch(HarvestCircleShellIntent.EnterReadOnly)
            presenter.dispatch(HarvestCircleShellIntent.Navigate(ShellDestination.Network))
            presenter.dispatch(HarvestCircleShellIntent.SetTheme(ThemePreference.Dark))
            presenter.dispatch(HarvestCircleShellIntent.SetMotion(MotionPreference.Reduced))
            presenter.dispatch(
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Open(FoundationOverlay.SyncStatus(org.harvestcircle.ui.shell.SyncStatusLabel.Degraded)),
                ),
            )

            assertEquals(AppRoute.Network, presenter.state.value.currentRoute)
            assertEquals(ThemePreference.Dark, presenter.state.value.appearance.theme)
            assertTrue(presenter.state.value.overlays.current is FoundationOverlay.SyncStatus)
            presenter.close()
        }

    @Test
    fun identityCommandsRemainOwnedByTheExistingPresenter() =
        runTest {
            val identity = FakeIdentityPresentation(presenterState(HarvestCircleRoute.IDENTITIES))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            presenter.dispatch(HarvestCircleShellIntent.Identity(HarvestCircleIntent.ChooseCreateIdentity))
            assertEquals(1, identity.intents.size)
            assertTrue(identity.intents.single() === HarvestCircleIntent.ChooseCreateIdentity)
            presenter.close()
        }

    @Test
    fun usableDegradationPreservesDashboardNavigation() =
        runTest {
            val identity = FakeIdentityPresentation(activePresenterState(ApplicationLifecycle.Ready, null, 1UL))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()
            presenter.dispatch(HarvestCircleShellIntent.Navigate(ShellDestination.Network))

            identity.state.value =
                activePresenterState(
                    ApplicationLifecycle.Degraded,
                    problem(ApplicationErrorCategory.Network),
                    2UL,
                )
            runCurrent()

            assertEquals(AppRoute.Network, presenter.state.value.currentRoute)
            assertEquals(
                LocalUsability.UsableDegraded(DegradationReason.Network),
                presenter.state.value.localUsability,
            )
            presenter.close()
        }

    @Test
    fun storageDegradationLeavesTheDashboardForALifecycleCanvas() =
        runTest {
            val identity = FakeIdentityPresentation(activePresenterState(ApplicationLifecycle.Ready, null, 1UL))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()

            identity.state.value =
                activePresenterState(
                    ApplicationLifecycle.Degraded,
                    problem(ApplicationErrorCategory.Storage),
                    2UL,
                )
            runCurrent()

            assertEquals(LocalUsability.Unusable, presenter.state.value.localUsability)
            assertTrue(presenter.state.value.root is ShellRoot.LifecycleCanvas)
            presenter.close()
        }
}

private class FakeIdentityPresentation(
    initial: HarvestCirclePresenterState,
) : IdentityPresentationPort {
    override val state = MutableStateFlow(initial)
    val intents = mutableListOf<HarvestCircleIntent>()

    override fun dispatch(intent: HarvestCircleIntent) {
        intents += intent
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

private fun activePresenterState(
    lifecycle: ApplicationLifecycle,
    lifecycleProblem: ApplicationProblem?,
    revision: ULong,
): HarvestCirclePresenterState {
    val identity =
        IdentitySummary(
            id = IdentityId.fromPublicKeyHex("02".repeat(32)),
            npub = "npub1active",
            displayLabel = "Active identity",
            signer = SignerBindingSummary(SignerBindingKind.LocalKeyring, SignerAvailability.Available),
            createdAt = UnixSeconds(1),
            lastUsedAt = null,
        )
    return HarvestCirclePresenterState(
        ApplicationSnapshot(
            revision = SnapshotRevision(revision),
            lifecycle = lifecycle,
            lifecycleProblem = lifecycleProblem,
            configuredRelays = emptyList(),
            identities = listOf(identity),
            selectedIdentityId = identity.id,
            session = SessionLifecycle.Active,
            sessionSubjectIdentityId = identity.id,
            sessionProblem = null,
            activeIdentity =
                ActiveIdentity(
                    identity = identity,
                    relays = RelaySummary(emptyList(), RelayConnectionState.Degraded),
                    profileState = ProfileLoadState.Cached,
                    profile = null,
                ),
            recoverableProblem = null,
        ),
    )
}

private fun problem(category: ApplicationErrorCategory): ApplicationProblem =
    ApplicationProblem(
        code =
            if (category == ApplicationErrorCategory.Network) {
                ApplicationErrorCode.RelayConnectionFailed
            } else {
                ApplicationErrorCode.StorageUnavailable
            },
        category = category,
        retryable = true,
        recoveryAction = RecoveryAction.Retry,
        operationId = null,
        safeMessage = "Typed degraded problem.",
    )
