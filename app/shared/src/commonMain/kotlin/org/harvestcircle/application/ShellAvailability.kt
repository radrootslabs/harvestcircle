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

enum class DegradationReason {
    Network,
    Credential,
    NetworkAndCredential,
}

sealed interface LocalUsability {
    data object Usable : LocalUsability

    data class UsableDegraded(
        val reason: DegradationReason,
    ) : LocalUsability

    data object Unusable : LocalUsability
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
): ShellRoot {
    if (
        presenterState.route !in
        setOf(
            HarvestCircleRoute.IDENTITIES,
            HarvestCircleRoute.ACTIVE_IDENTITY,
            HarvestCircleRoute.DEGRADED,
        )
    ) {
        return ShellRoot.LifecycleCanvas(presenterState.route)
    }
    if (deriveLocalUsability(presenterState.snapshot) is LocalUsability.Unusable) {
        return ShellRoot.LifecycleCanvas(presenterState.route)
    }
    if (presenterState.generatedKeyBackup != null) {
        return ShellRoot.BootstrapCanvas(BootstrapStep.GeneratedRecovery)
    }
    if (presenterState.snapshot.activeIdentity != null || session.readOnly) {
        return ShellRoot.Dashboard(NavigationState(AppRoute.PersonalToday))
    }
    val step =
        when (presenterState.identityEntryMode) {
            IdentityEntryMode.CREATE -> BootstrapStep.CreateIdentity
            IdentityEntryMode.IMPORT -> BootstrapStep.ImportIdentity
            IdentityEntryMode.CHOICE ->
                if (presenterState.snapshot.identities.isEmpty()) {
                    BootstrapStep.Welcome
                } else {
                    BootstrapStep.IdentityChooser
                }
        }
    return ShellRoot.BootstrapCanvas(step)
}

fun deriveLocalUsability(snapshot: ApplicationSnapshot): LocalUsability =
    when (snapshot.lifecycle) {
        ApplicationLifecycle.Ready -> LocalUsability.Usable
        ApplicationLifecycle.Degraded -> deriveDegradedUsability(snapshot)
        else -> LocalUsability.Unusable
    }

private fun deriveDegradedUsability(snapshot: ApplicationSnapshot): LocalUsability {
    val categories =
        listOfNotNull(
            snapshot.lifecycleProblem,
            snapshot.sessionProblem,
            snapshot.recoverableProblem,
        ).map(ApplicationProblem::category)
            .toSet()
    if (categories.isEmpty() || categories.any { it !in USABLE_DEGRADATION_CATEGORIES }) {
        return LocalUsability.Unusable
    }
    val reason =
        when (categories) {
            setOf(ApplicationErrorCategory.Network) -> DegradationReason.Network
            setOf(ApplicationErrorCategory.Credential) -> DegradationReason.Credential
            else -> DegradationReason.NetworkAndCredential
        }
    return LocalUsability.UsableDegraded(reason)
}

private val USABLE_DEGRADATION_CATEGORIES =
    setOf(
        ApplicationErrorCategory.Network,
        ApplicationErrorCategory.Credential,
    )

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
