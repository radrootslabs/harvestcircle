package org.harvestcircle.navigation

import org.harvestcircle.product.ScreenKey
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
    val screenKey: ScreenKey

    data class Bootstrap(
        val step: BootstrapStep,
    ) : AppRoute {
        override val screenKey: ScreenKey = ScreenKey.Bootstrap
    }

    data object PersonalToday : AppRoute {
        override val screenKey: ScreenKey = ScreenKey.PersonalToday
    }

    data object Network : AppRoute {
        override val screenKey: ScreenKey = ScreenKey.Network
    }

    data object Settings : AppRoute {
        override val screenKey: ScreenKey = ScreenKey.Settings
    }
}

data class SettingsUiState(
    val section: SettingsSection = SettingsSection.Appearance,
)

data class NavigationState(
    val current: AppRoute,
    val backStack: List<AppRoute> = emptyList(),
    val forwardStack: List<AppRoute> = emptyList(),
    val workspace: WorkspaceKind = current.workspace(),
    val settingsReturnRoute: AppRoute? = null,
    val settings: SettingsUiState = SettingsUiState(),
) {
    init {
        require(backStack.size <= HISTORY_LIMIT && forwardStack.size <= HISTORY_LIMIT)
        require(backStack.none(AppRoute::isTransient))
        require(forwardStack.none(AppRoute::isTransient))
        require(settingsReturnRoute !is AppRoute.Bootstrap && settingsReturnRoute != AppRoute.Settings)
    }
}

sealed interface NavigationIntent {
    data class Navigate(
        val route: AppRoute,
    ) : NavigationIntent

    data class SelectBootstrapStep(
        val step: BootstrapStep,
    ) : NavigationIntent

    data class SelectSettingsSection(
        val section: SettingsSection,
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
            is NavigationIntent.SelectSettingsSection -> selectSettingsSection(state, intent.section)
            NavigationIntent.Back -> moveBack(state)
            NavigationIntent.Forward -> moveForward(state)
            NavigationIntent.ReturnFromSettings -> returnFromSettings(state)
        }

    private fun navigate(
        state: NavigationState,
        route: AppRoute,
    ): NavigationState {
        if (route is AppRoute.Bootstrap || route == state.current) return state
        val enteringSettings = route == AppRoute.Settings
        val leavingSettings = state.current == AppRoute.Settings
        val prior = state.current.takeUnless(AppRoute::isTransient)
        val existingRouteIndex = state.backStack.indexOfLast { it == route }
        val history =
            when {
                leavingSettings && existingRouteIndex >= 0 -> state.backStack.take(existingRouteIndex)
                leavingSettings -> state.backStack
                else -> prior?.let { (state.backStack + it).takeLast(HISTORY_LIMIT) }.orEmpty()
            }
        return state.copy(
            current = route,
            backStack = history,
            forwardStack = emptyList(),
            workspace = route.workspace(),
            settingsReturnRoute = if (enteringSettings) state.current else null,
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

    private fun selectSettingsSection(
        state: NavigationState,
        section: SettingsSection,
    ): NavigationState =
        if (state.current == AppRoute.Settings) {
            state.copy(settings = state.settings.copy(section = section))
        } else {
            state
        }

    private fun moveBack(state: NavigationState): NavigationState {
        if (state.current == AppRoute.Settings) return returnFromSettings(state)
        val destination = state.backStack.lastOrNull() ?: return state
        return state.copy(
            current = destination,
            backStack = state.backStack.dropLast(1),
            forwardStack = (listOf(state.current) + state.forwardStack).take(HISTORY_LIMIT),
            workspace = destination.workspace(),
            settingsReturnRoute = null,
        )
    }

    private fun moveForward(state: NavigationState): NavigationState {
        val destination = state.forwardStack.firstOrNull() ?: return state
        return state.copy(
            current = destination,
            backStack = (state.backStack + state.current).takeLast(HISTORY_LIMIT),
            forwardStack = state.forwardStack.drop(1),
            workspace = destination.workspace(),
            settingsReturnRoute = if (destination == AppRoute.Settings) state.current else null,
        )
    }

    private fun returnFromSettings(state: NavigationState): NavigationState {
        if (state.current != AppRoute.Settings) return state
        val destination = state.settingsReturnRoute ?: state.backStack.lastOrNull() ?: return state
        val backStack =
            if (state.backStack.lastOrNull() == destination) {
                state.backStack.dropLast(1)
            } else {
                state.backStack
            }
        return state.copy(
            current = destination,
            backStack = backStack,
            forwardStack = state.forwardStack,
            workspace = destination.workspace(),
            settingsReturnRoute = null,
        )
    }
}

fun ScreenKey.toExecutableRoute(): AppRoute? =
    when (this) {
        ScreenKey.PersonalToday -> AppRoute.PersonalToday
        ScreenKey.Network -> AppRoute.Network
        ScreenKey.Settings -> AppRoute.Settings
        else -> null
    }

private fun AppRoute.workspace(): WorkspaceKind = screenKey.descriptor.workspace

private fun AppRoute.isTransient(): Boolean = this is AppRoute.Bootstrap || this == AppRoute.Settings

private const val HISTORY_LIMIT = 32
