package org.harvestcircle.application

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.NavigationIntent

interface IdentityPresentationPort {
    val state: StateFlow<HarvestCirclePresenterState>

    fun dispatch(intent: HarvestCircleIntent)
}

data class HarvestCircleShellState(
    val identity: HarvestCirclePresenterState,
    val buildInfo: BuildInfo,
    val session: ShellSessionState = ShellSessionState(),
    val localUsability: LocalUsability = deriveLocalUsability(identity.snapshot),
    val root: ShellRoot = deriveShellRoot(identity, session),
    val appearance: AppearanceState = AppearanceState(),
    val overlays: OverlayState = OverlayState(),
)

sealed interface HarvestCircleShellIntent {
    data class Identity(
        val intent: HarvestCircleIntent,
    ) : HarvestCircleShellIntent

    data object EnterReadOnly : HarvestCircleShellIntent

    data class Navigate(
        val destination: ShellDestination,
    ) : HarvestCircleShellIntent

    data class Navigation(
        val intent: NavigationIntent,
    ) : HarvestCircleShellIntent

    data class Overlay(
        val intent: OverlayIntent,
    ) : HarvestCircleShellIntent

    data class SetTheme(
        val theme: ThemePreference,
    ) : HarvestCircleShellIntent

    data class SetTextSize(
        val textSize: TextSizePreference,
    ) : HarvestCircleShellIntent

    data class SetMotion(
        val motion: MotionPreference,
    ) : HarvestCircleShellIntent
}

class HarvestCircleShellPresenter(
    private val identityPresenter: IdentityPresentationPort,
    buildInfo: BuildInfo,
    scope: CoroutineScope,
) {
    private val mutableState = MutableStateFlow(HarvestCircleShellState(identityPresenter.state.value, buildInfo))
    val state: StateFlow<HarvestCircleShellState> = mutableState.asStateFlow()
    private val observation: Job =
        scope.launch {
            identityPresenter.state.collect { identity -> updateIdentity(identity) }
        }

    fun dispatch(intent: HarvestCircleShellIntent) {
        when (intent) {
            is HarvestCircleShellIntent.Identity -> identityPresenter.dispatch(intent.intent)
            HarvestCircleShellIntent.EnterReadOnly -> reduce(ShellEvent.EnterReadOnly)
            is HarvestCircleShellIntent.Navigate -> reduce(ShellEvent.Navigate(intent.destination))
            is HarvestCircleShellIntent.Navigation -> reduce(ShellEvent.Navigation(intent.intent))
            is HarvestCircleShellIntent.Overlay -> reduce(ShellEvent.Overlay(intent.intent))
            is HarvestCircleShellIntent.SetTheme -> reduce(ShellEvent.SetTheme(intent.theme))
            is HarvestCircleShellIntent.SetTextSize -> reduce(ShellEvent.SetTextSize(intent.textSize))
            is HarvestCircleShellIntent.SetMotion -> reduce(ShellEvent.SetMotion(intent.motion))
        }
    }

    fun close() {
        observation.cancel()
    }

    private fun updateIdentity(identity: HarvestCirclePresenterState) {
        reduce(ShellEvent.IdentityObserved(identity))
    }

    private fun reduce(event: ShellEvent) {
        mutableState.update { current -> ShellReducer.reduce(current, event) }
    }
}

val HarvestCircleShellState.currentRoute: AppRoute?
    get() = (root as? ShellRoot.Dashboard)?.navigation?.current
