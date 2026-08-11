package org.harvestcircle.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.product.ScreenKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class ShellReducerTest {
    @Test
    fun reducerRetainsIndependentStateAcrossDeterministicInterleavings() {
        val initial = HarvestCircleShellState(activePresenterState(1UL), BuildInfo.unknown())
        val events =
            listOf(
                ShellEvent.IdentityObserved(activePresenterState(3UL)),
                ShellEvent.Navigate(ScreenKey.Network),
                ShellEvent.SetTheme(ThemePreference.Dark),
                ShellEvent.SetTextSize(TextSizePreference.VeryLarge),
                ShellEvent.SetMotion(MotionPreference.Reduced),
                ShellEvent.Overlay(OverlayIntent.Open(FoundationOverlay.OpenNostrReference())),
            )

        val forward = events.fold(initial, ShellReducer::reduce)
        val reverse = events.reversed().fold(initial, ShellReducer::reduce)

        listOf(forward, reverse).forEach { state ->
            assertEquals(3UL, state.identity.snapshot.revision.value)
            assertEquals(AppRoute.Network, state.currentRoute)
            assertEquals(ThemePreference.Dark, state.appearance.theme)
            assertEquals(TextSizePreference.VeryLarge, state.appearance.textSize)
            assertEquals(MotionPreference.Reduced, state.appearance.motion)
            assertTrue(state.overlays.current is FoundationOverlay.OpenNostrReference)
        }
    }

    @Test
    fun reducerRejectsStaleIdentityObservations() {
        val current = HarvestCircleShellState(activePresenterState(3UL), BuildInfo.unknown())
        val reduced = ShellReducer.reduce(current, ShellEvent.IdentityObserved(activePresenterState(2UL)))
        assertTrue(reduced === current)
    }

    @Test
    fun presenterAtomicallyRetainsConcurrentIndependentEvents() =
        runTest {
            val identity = ReducerIdentityPresentation(activePresenterState(1UL))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()

            listOf(
                HarvestCircleShellIntent.Navigate(ScreenKey.Network),
                HarvestCircleShellIntent.SetTheme(ThemePreference.Dark),
                HarvestCircleShellIntent.SetTextSize(TextSizePreference.VeryLarge),
                HarvestCircleShellIntent.SetMotion(MotionPreference.Reduced),
                HarvestCircleShellIntent.EnterReadOnly,
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Open(FoundationOverlay.OpenNostrReference()),
                ),
            ).map { intent -> async { presenter.dispatch(intent) } }
                .awaitAll()

            val state = presenter.state.value
            assertEquals(AppRoute.Network, state.currentRoute)
            assertEquals(ThemePreference.Dark, state.appearance.theme)
            assertEquals(TextSizePreference.VeryLarge, state.appearance.textSize)
            assertEquals(MotionPreference.Reduced, state.appearance.motion)
            assertTrue(state.session.readOnly)
            assertTrue(state.overlays.current is FoundationOverlay.OpenNostrReference)
            presenter.close()
        }

    @Test
    fun observationCloseIsIdempotentAndTerminal() =
        runTest {
            val identity = ReducerIdentityPresentation(activePresenterState(1UL))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()

            presenter.close()
            presenter.close()
            identity.state.value = activePresenterState(2UL)
            runCurrent()

            assertEquals(1UL, presenter.state.value.identity.snapshot.revision.value)
        }
}

private class ReducerIdentityPresentation(
    initial: HarvestCirclePresenterState,
) : IdentityPresentationPort {
    override val state = MutableStateFlow(initial)

    override fun dispatch(intent: HarvestCircleIntent) = Unit
}

private fun activePresenterState(revision: ULong): HarvestCirclePresenterState {
    val identity =
        IdentitySummary(
            id = IdentityId.fromPublicKeyHex("03".repeat(32)),
            npub = "npub1reducer",
            displayLabel = "Reducer identity",
            signer = SignerBindingSummary(SignerBindingKind.LocalKeyring, SignerAvailability.Available),
            createdAt = UnixSeconds(1),
            lastUsedAt = null,
        )
    return HarvestCirclePresenterState(
        ApplicationSnapshot(
            revision = SnapshotRevision(revision),
            lifecycle = ApplicationLifecycle.Ready,
            lifecycleProblem = null,
            configuredRelays = emptyList(),
            identities = listOf(identity),
            selectedIdentityId = identity.id,
            session = SessionLifecycle.Active,
            sessionSubjectIdentityId = identity.id,
            sessionProblem = null,
            activeIdentity =
                ActiveIdentity(
                    identity = identity,
                    relays = RelaySummary(emptyList(), RelayConnectionState.Disconnected),
                    profileState = ProfileLoadState.Empty,
                    profile = null,
                ),
            recoverableProblem = null,
        ),
    )
}
