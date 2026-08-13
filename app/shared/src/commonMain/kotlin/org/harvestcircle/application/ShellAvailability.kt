package org.harvestcircle.application

import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.BootstrapStep
import org.harvestcircle.navigation.NavigationIntent
import org.harvestcircle.navigation.NavigationReducer
import org.harvestcircle.navigation.NavigationState
import org.harvestcircle.navigation.toExecutableRoute
import org.harvestcircle.product.FeatureAvailability
import org.harvestcircle.product.NavigationKind
import org.harvestcircle.product.ScreenKey

data class ShellSessionState(
    val readOnly: Boolean = false,
) {
    fun enterReadOnly(): ShellSessionState = copy(readOnly = true)

    fun leaveReadOnly(): ShellSessionState = copy(readOnly = false)
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

data class ShellNavigationItem(
    val screenKey: ScreenKey,
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

private val PERMANENT_SHELL_SCREEN_ORDER =
    listOf(
        ScreenKey.PersonalToday,
        ScreenKey.Explore,
        ScreenKey.Activity,
        ScreenKey.Network,
    ).also { keys -> require(keys.all { it.descriptor.navigation == NavigationKind.Permanent }) }

val shellNavigationItems: List<ShellNavigationItem> =
    PERMANENT_SHELL_SCREEN_ORDER.map(::navigationItem)

val shellSettingsItem: ShellNavigationItem = navigationItem(ScreenKey.Settings)

data class ShellWorkspaceAction(
    val label: String,
    val enabled: Boolean,
    val unavailableExplanation: String,
)

val addFarmWorkspaceAction =
    ShellWorkspaceAction(
        label = "Add a farm workspace",
        enabled = false,
        unavailableExplanation = "Not available in this build.",
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
    if (presenterState.activatingIdentityId != null) {
        return ShellRoot.BootstrapCanvas(BootstrapStep.ActivationProgress)
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

fun activateShellScreen(
    state: NavigationState,
    screenKey: ScreenKey,
): NavigationState {
    val item = (shellNavigationItems + shellSettingsItem).singleOrNull { it.screenKey == screenKey } ?: return state
    return item.route?.let { NavigationReducer.reduce(state, NavigationIntent.Navigate(it)) } ?: state
}

private fun navigationItem(screenKey: ScreenKey): ShellNavigationItem {
    val route = screenKey.toExecutableRoute()
    val enabled = screenKey.descriptor.availability == FeatureAvailability.Foundation && route != null
    return ShellNavigationItem(
        screenKey = screenKey,
        label = screenKey.label(),
        enabled = enabled,
        unavailableExplanation = if (enabled) null else "Not available in this build.",
        route = route.takeIf { enabled },
    )
}

private fun ScreenKey.label(): String =
    when (this) {
        ScreenKey.PersonalToday -> "Today"
        ScreenKey.Explore -> "Explore"
        ScreenKey.Activity -> "Activity"
        ScreenKey.Network -> "Network"
        ScreenKey.Settings -> "Settings"
        else -> error("Screen is not presented in shell navigation: ${descriptor.externalKey}")
    }
