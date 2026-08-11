package org.harvestcircle.application

import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.navigation.NavigationIntent
import org.harvestcircle.navigation.NavigationReducer

sealed interface ShellEvent {
    data class IdentityObserved(
        val identity: HarvestCirclePresenterState,
    ) : ShellEvent

    data object EnterReadOnly : ShellEvent

    data class Navigate(
        val destination: ShellDestination,
    ) : ShellEvent

    data class Navigation(
        val intent: NavigationIntent,
    ) : ShellEvent

    data class Overlay(
        val intent: OverlayIntent,
    ) : ShellEvent

    data class SetTheme(
        val theme: ThemePreference,
    ) : ShellEvent

    data class SetTextSize(
        val textSize: TextSizePreference,
    ) : ShellEvent

    data class SetMotion(
        val motion: MotionPreference,
    ) : ShellEvent
}

object ShellReducer {
    fun reduce(
        state: HarvestCircleShellState,
        event: ShellEvent,
    ): HarvestCircleShellState =
        when (event) {
            is ShellEvent.IdentityObserved -> observeIdentity(state, event.identity)
            ShellEvent.EnterReadOnly -> updateSession(state, state.session.enterReadOnly())
            is ShellEvent.Navigate -> updateNavigation(state) { activateShellDestination(it, event.destination) }
            is ShellEvent.Navigation -> updateNavigation(state, event.intent)
            is ShellEvent.Overlay -> state.copy(overlays = OverlayReducer.reduce(state.overlays, event.intent))
            is ShellEvent.SetTheme -> state.copy(appearance = state.appearance.copy(theme = event.theme))
            is ShellEvent.SetTextSize -> state.copy(appearance = state.appearance.copy(textSize = event.textSize))
            is ShellEvent.SetMotion -> state.copy(appearance = state.appearance.copy(motion = event.motion))
        }

    private fun observeIdentity(
        state: HarvestCircleShellState,
        identity: HarvestCirclePresenterState,
    ): HarvestCircleShellState {
        if (identity.snapshot.revision.value < state.identity.snapshot.revision.value) return state
        val derived = deriveShellRoot(identity, state.session)
        return state.copy(
            identity = identity,
            localUsability = deriveLocalUsability(identity.snapshot),
            root = retainDashboardNavigation(state.root, derived),
        )
    }

    private fun updateSession(
        state: HarvestCircleShellState,
        session: ShellSessionState,
    ): HarvestCircleShellState {
        val derived = deriveShellRoot(state.identity, session)
        return state.copy(
            session = session,
            root = retainDashboardNavigation(state.root, derived),
        )
    }

    private fun updateNavigation(
        state: HarvestCircleShellState,
        block: (org.harvestcircle.navigation.NavigationState) -> org.harvestcircle.navigation.NavigationState,
    ): HarvestCircleShellState {
        val dashboard = state.root as? ShellRoot.Dashboard ?: return state
        return state.copy(root = dashboard.copy(navigation = block(dashboard.navigation)))
    }

    private fun updateNavigation(
        state: HarvestCircleShellState,
        intent: NavigationIntent,
    ): HarvestCircleShellState =
        when (val root = state.root) {
            is ShellRoot.Dashboard ->
                state.copy(root = root.copy(navigation = NavigationReducer.reduce(root.navigation, intent)))
            is ShellRoot.BootstrapCanvas ->
                if (intent is NavigationIntent.SelectBootstrapStep) {
                    state.copy(root = root.copy(step = intent.step))
                } else {
                    state
                }
            is ShellRoot.LifecycleCanvas -> state
        }

    private fun retainDashboardNavigation(
        current: ShellRoot,
        derived: ShellRoot,
    ): ShellRoot =
        if (current is ShellRoot.Dashboard && derived is ShellRoot.Dashboard) {
            derived.copy(navigation = current.navigation)
        } else {
            derived
        }
}
