package org.harvestcircle.application

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch
import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.NavigationIntent
import org.harvestcircle.navigation.NavigationReducer
import org.harvestcircle.navigation.NavigationState

interface IdentityPresentationPort {
    val state: StateFlow<HarvestCirclePresenterState>

    fun dispatch(intent: HarvestCircleIntent)
}

data class HarvestCircleShellState(
    val identity: HarvestCirclePresenterState,
    val buildInfo: BuildInfo,
    val session: ShellSessionState = ShellSessionState(),
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
            HarvestCircleShellIntent.EnterReadOnly -> updateSession(mutableState.value.session.enterReadOnly())
            is HarvestCircleShellIntent.Navigate -> updateNavigation { activateShellDestination(it, intent.destination) }
            is HarvestCircleShellIntent.Navigation -> updateNavigationIntent(intent.intent)
            is HarvestCircleShellIntent.Overlay -> mutate { copy(overlays = OverlayReducer.reduce(overlays, intent.intent)) }
            is HarvestCircleShellIntent.SetTheme -> mutate { copy(appearance = appearance.copy(theme = intent.theme)) }
            is HarvestCircleShellIntent.SetTextSize -> mutate { copy(appearance = appearance.copy(textSize = intent.textSize)) }
            is HarvestCircleShellIntent.SetMotion -> mutate { copy(appearance = appearance.copy(motion = intent.motion)) }
        }
    }

    fun close() {
        observation.cancel()
    }

    private fun updateIdentity(identity: HarvestCirclePresenterState) {
        mutate {
            val derived = deriveShellRoot(identity, session)
            val retained =
                when {
                    derived is ShellRoot.Dashboard && root is ShellRoot.Dashboard ->
                        derived.copy(navigation = root.navigation)
                    else -> derived
                }
            copy(identity = identity, root = retained)
        }
    }

    private fun updateSession(session: ShellSessionState) {
        mutate { copy(session = session, root = deriveShellRoot(identity, session)) }
    }

    private fun updateNavigation(block: (NavigationState) -> NavigationState) {
        mutate {
            val dashboard = root as? ShellRoot.Dashboard ?: return@mutate this
            copy(root = dashboard.copy(navigation = block(dashboard.navigation)))
        }
    }

    private fun updateNavigationIntent(intent: NavigationIntent) {
        mutate {
            when (val currentRoot = root) {
                is ShellRoot.Dashboard ->
                    copy(
                        root = currentRoot.copy(navigation = NavigationReducer.reduce(currentRoot.navigation, intent)),
                    )
                is ShellRoot.BootstrapCanvas ->
                    if (intent is NavigationIntent.SelectBootstrapStep) {
                        copy(root = currentRoot.copy(step = intent.step))
                    } else {
                        this
                    }
                is ShellRoot.LifecycleCanvas -> this
            }
        }
    }

    private fun mutate(block: HarvestCircleShellState.() -> HarvestCircleShellState) {
        mutableState.value = mutableState.value.block()
    }
}

val HarvestCircleShellState.currentRoute: AppRoute?
    get() = (root as? ShellRoot.Dashboard)?.navigation?.current
