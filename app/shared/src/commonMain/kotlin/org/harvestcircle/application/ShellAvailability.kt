package org.harvestcircle.application

import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.BootstrapStep
import org.harvestcircle.navigation.NavigationIntent
import org.harvestcircle.navigation.NavigationReducer
import org.harvestcircle.navigation.NavigationState
import org.harvestcircle.navigation.SettingsSection

data class ShellSessionState(
    val readOnly: Boolean = false,
) {
    fun enterReadOnly(): ShellSessionState = copy(readOnly = true)
}

enum class ShellDestination { Today, Explore, Activity, Network, Settings, AddFarm }

data class ShellNavigationItem(
    val destination: ShellDestination,
    val label: String,
    val enabled: Boolean,
    val unavailableExplanation: String? = null,
    val route: AppRoute? = null,
) {
    init {
        require(enabled == (route != null))
        require(enabled || !unavailableExplanation.isNullOrBlank())
    }
}

val shellNavigationItems: List<ShellNavigationItem> =
    listOf(
        ShellNavigationItem(ShellDestination.Today, "Today", true, route = AppRoute.PersonalToday),
        unavailable(ShellDestination.Explore, "Explore"),
        unavailable(ShellDestination.Activity, "Activity"),
        ShellNavigationItem(ShellDestination.Network, "Network", true, route = AppRoute.Network),
        ShellNavigationItem(
            ShellDestination.Settings,
            "Settings",
            true,
            route = AppRoute.Settings(SettingsSection.Appearance),
        ),
        unavailable(ShellDestination.AddFarm, "Add a farm workspace"),
    )

sealed interface ShellRoot {
    data class LifecycleCanvas(
        val route: HarvestCircleRoute,
    ) : ShellRoot

    data class BootstrapCanvas(
        val step: BootstrapStep,
    ) : ShellRoot

    data class Dashboard(
        val navigation: NavigationState,
    ) : ShellRoot
}

fun deriveShellRoot(
    presenterState: HarvestCirclePresenterState,
    session: ShellSessionState,
): ShellRoot =
    when (presenterState.route) {
        HarvestCircleRoute.ACTIVE_IDENTITY -> ShellRoot.Dashboard(NavigationState(AppRoute.PersonalToday))
        HarvestCircleRoute.IDENTITIES ->
            if (session.readOnly) {
                ShellRoot.Dashboard(NavigationState(AppRoute.PersonalToday))
            } else {
                val step =
                    if (presenterState.snapshot.identities.isEmpty()) {
                        BootstrapStep.Welcome
                    } else {
                        BootstrapStep.IdentityChooser
                    }
                ShellRoot.BootstrapCanvas(step)
            }
        else -> ShellRoot.LifecycleCanvas(presenterState.route)
    }

fun activateShellDestination(
    state: NavigationState,
    destination: ShellDestination,
): NavigationState {
    val item = shellNavigationItems.single { it.destination == destination }
    return item.route?.let { NavigationReducer.reduce(state, NavigationIntent.Navigate(it)) } ?: state
}

private fun unavailable(
    destination: ShellDestination,
    label: String,
): ShellNavigationItem =
    ShellNavigationItem(
        destination = destination,
        label = label,
        enabled = false,
        unavailableExplanation = "Available after collective contracts are implemented.",
    )
