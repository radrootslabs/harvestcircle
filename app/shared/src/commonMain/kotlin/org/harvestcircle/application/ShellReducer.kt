package org.harvestcircle.application

import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.navigation.NavigationIntent
import org.harvestcircle.navigation.NavigationReducer
import org.harvestcircle.product.ScreenKey

sealed interface ShellEvent {
    data class IdentityObserved(
        val identity: HarvestCirclePresenterState,
    ) : ShellEvent

    data object EnterReadOnly : ShellEvent

    data class Navigate(
        val screenKey: ScreenKey,
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
            is ShellEvent.Navigate -> updateNavigation(state) { activateShellScreen(it, event.screenKey) }
            is ShellEvent.Navigation -> updateNavigation(state, event.intent)
            is ShellEvent.Overlay -> OverlayReducer.transition(state, event.intent).state
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
            overlays = reconcileRemovalOverlay(state, identity),
        )
    }

    private fun reconcileRemovalOverlay(
        state: HarvestCircleShellState,
        identity: HarvestCirclePresenterState,
    ): OverlayState {
        val admitted = identity.removalConfirmation
        val current = state.overlays.current as? FoundationOverlay.ConfirmAction
        if (admitted == null) {
            return if (current?.action is ConfirmationAction.RemoveLocalIdentity) {
                state.overlays.copy(current = null)
            } else {
                state.overlays
            }
        }
        val action = ConfirmationAction.RemoveLocalIdentity(admitted.identityId, admitted.requestId)
        if (current?.action == action) {
            val admissionRejected =
                current.busy &&
                    !identity.busy &&
                    identity.removalStatus == RemovalStatus.AWAITING_CONFIRMATION &&
                    identity.commandStatus in REMOVAL_ADMISSION_FAILURES
            return if (admissionRejected) {
                state.overlays.copy(current = current.copy(phase = ConfirmationPhase.Ready))
            } else {
                state.overlays
            }
        }
        val impact =
            when {
                admitted.deletesLocalCredential && admitted.signsOut ->
                    "Its local credential will be deleted and the active session will be signed out."
                admitted.deletesLocalCredential ->
                    "Its local credential will be deleted from the operating-system keyring."
                admitted.signsOut -> "The active session will be signed out."
                else -> "This saved local identity will be removed."
            }
        return OverlayState(
            FoundationOverlay.ConfirmAction(
                title = "Remove this saved identity?",
                explanation = impact,
                actionLabel = "Remove local identity",
                action = action,
            ),
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

private val REMOVAL_ADMISSION_FAILURES =
    setOf(
        CommandStatus.REJECTED_BUSY,
        CommandStatus.REJECTED_CLOSED,
        CommandStatus.FAILED_RETRYABLE,
        CommandStatus.FAILED_TERMINAL,
    )
