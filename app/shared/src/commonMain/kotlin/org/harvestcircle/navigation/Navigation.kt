package org.harvestcircle.navigation

import org.harvestcircle.product.WorkspaceKind

enum class BootstrapStep {
    Welcome,
    CreateIdentity,
    ImportIdentity,
    GeneratedRecovery,
    IdentityChooser,
    ActivationProgress,
}

enum class SettingsSection { Appearance, Project }

sealed interface AppRoute {
    data class Bootstrap(
        val step: BootstrapStep,
    ) : AppRoute

    data object PersonalToday : AppRoute

    data object Network : AppRoute

    data class Settings(
        val section: SettingsSection,
    ) : AppRoute
}

data class NavigationState(
    val current: AppRoute,
    val backStack: List<AppRoute> = emptyList(),
    val forwardStack: List<AppRoute> = emptyList(),
    val workspace: WorkspaceKind = current.workspace(),
    val settingsReturnRoute: AppRoute? = null,
) {
    init {
        require(backStack.size <= HISTORY_LIMIT && forwardStack.size <= HISTORY_LIMIT)
        require(backStack.none { it is AppRoute.Bootstrap })
        require(forwardStack.none { it is AppRoute.Bootstrap })
    }
}

sealed interface NavigationIntent {
    data class Navigate(
        val route: AppRoute,
    ) : NavigationIntent

    data class SelectBootstrapStep(
        val step: BootstrapStep,
    ) : NavigationIntent

    data object Back : NavigationIntent

    data object Forward : NavigationIntent

    data object ReturnFromSettings : NavigationIntent
}

object NavigationReducer {
    fun reduce(
        state: NavigationState,
        intent: NavigationIntent,
    ): NavigationState =
        when (intent) {
            is NavigationIntent.Navigate -> navigate(state, intent.route)
            is NavigationIntent.SelectBootstrapStep -> selectBootstrapStep(state, intent.step)
            NavigationIntent.Back -> moveBack(state)
            NavigationIntent.Forward -> moveForward(state)
            NavigationIntent.ReturnFromSettings -> state.settingsReturnRoute?.let { navigate(state, it) } ?: state
        }

    private fun navigate(
        state: NavigationState,
        route: AppRoute,
    ): NavigationState {
        if (route is AppRoute.Bootstrap || route == state.current) return state
        val returnRoute =
            when {
                route is AppRoute.Settings && state.current !is AppRoute.Settings -> state.current
                route !is AppRoute.Settings -> null
                else -> state.settingsReturnRoute
            }
        val prior = state.current.takeUnless { it is AppRoute.Bootstrap }
        return state.copy(
            current = route,
            backStack = prior?.let { (state.backStack + it).takeLast(HISTORY_LIMIT) }.orEmpty(),
            forwardStack = emptyList(),
            workspace = route.workspace(),
            settingsReturnRoute = returnRoute,
        )
    }

    private fun selectBootstrapStep(
        state: NavigationState,
        step: BootstrapStep,
    ): NavigationState =
        if (state.current is AppRoute.Bootstrap) {
            state.copy(current = AppRoute.Bootstrap(step), workspace = WorkspaceKind.System)
        } else {
            state
        }

    private fun moveBack(state: NavigationState): NavigationState {
        val destination = state.backStack.lastOrNull() ?: return state
        return state.copy(
            current = destination,
            backStack = state.backStack.dropLast(1),
            forwardStack = (listOf(state.current) + state.forwardStack).take(HISTORY_LIMIT),
            workspace = destination.workspace(),
            settingsReturnRoute = if (destination is AppRoute.Settings) state.settingsReturnRoute else null,
        )
    }

    private fun moveForward(state: NavigationState): NavigationState {
        val destination = state.forwardStack.firstOrNull() ?: return state
        return state.copy(
            current = destination,
            backStack = (state.backStack + state.current).takeLast(HISTORY_LIMIT),
            forwardStack = state.forwardStack.drop(1),
            workspace = destination.workspace(),
            settingsReturnRoute = if (destination is AppRoute.Settings) state.current else null,
        )
    }
}

private fun AppRoute.workspace(): WorkspaceKind =
    when (this) {
        is AppRoute.Bootstrap -> WorkspaceKind.System
        AppRoute.PersonalToday -> WorkspaceKind.Personal
        AppRoute.Network, is AppRoute.Settings -> WorkspaceKind.Shared
    }

private const val HISTORY_LIMIT = 32
