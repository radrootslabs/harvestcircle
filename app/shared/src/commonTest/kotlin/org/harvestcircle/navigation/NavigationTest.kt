package org.harvestcircle.navigation

import org.harvestcircle.product.ScreenKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertSame

class NavigationTest {
    @Test
    fun navigationSupportsBackForwardAndClearsForward() {
        val today = NavigationState(AppRoute.PersonalToday)
        val network = NavigationReducer.reduce(today, NavigationIntent.Navigate(AppRoute.Network))
        val back = NavigationReducer.reduce(network, NavigationIntent.Back)
        assertEquals(AppRoute.PersonalToday, back.current)
        assertEquals(AppRoute.Network, NavigationReducer.reduce(back, NavigationIntent.Forward).current)
        val replaced = NavigationReducer.reduce(back, NavigationIntent.Navigate(AppRoute.Settings))
        assertEquals(emptyList(), replaced.forwardStack)
    }

    @Test
    fun duplicateAndUnavailableBootstrapNavigationAreNoOps() {
        val state = NavigationState(AppRoute.PersonalToday)
        assertSame(state, NavigationReducer.reduce(state, NavigationIntent.Navigate(AppRoute.PersonalToday)))
        assertSame(
            state,
            NavigationReducer.reduce(state, NavigationIntent.Navigate(AppRoute.Bootstrap(BootstrapStep.Welcome))),
        )
    }

    @Test
    fun bootstrapStepsDoNotEnterHistory() {
        val state = NavigationState(AppRoute.Bootstrap(BootstrapStep.Welcome))
        val create = NavigationReducer.reduce(state, NavigationIntent.SelectBootstrapStep(BootstrapStep.CreateIdentity))
        assertEquals(AppRoute.Bootstrap(BootstrapStep.CreateIdentity), create.current)
        assertEquals(emptyList(), create.backStack)
        assertEquals(emptyList(), create.forwardStack)
    }

    @Test
    fun historyIsBoundedAndSettingsReturnsToPriorRoute() {
        var state = NavigationState(AppRoute.PersonalToday)
        repeat(40) { index ->
            val route = if (index % 2 == 0) AppRoute.Network else AppRoute.PersonalToday
            state = NavigationReducer.reduce(state, NavigationIntent.Navigate(route))
        }
        assertEquals(32, state.backStack.size)
        state = NavigationReducer.reduce(state, NavigationIntent.Navigate(AppRoute.Settings))
        assertEquals(AppRoute.PersonalToday, NavigationReducer.reduce(state, NavigationIntent.ReturnFromSettings).current)
    }

    @Test
    fun settingsSectionsAreHistoryNeutralAndLeavingRemovesSettings() {
        val network = NavigationReducer.reduce(NavigationState(AppRoute.PersonalToday), NavigationIntent.Navigate(AppRoute.Network))
        val settings = NavigationReducer.reduce(network, NavigationIntent.Navigate(AppRoute.Settings))
        val project =
            NavigationReducer.reduce(
                settings,
                NavigationIntent.SelectSettingsSection(SettingsSection.Project),
            )
        assertEquals(SettingsSection.Project, project.settings.section)
        assertEquals(settings.backStack, project.backStack)

        val returned = NavigationReducer.reduce(project, NavigationIntent.Back)
        assertEquals(AppRoute.Network, returned.current)
        assertEquals(listOf(AppRoute.PersonalToday), returned.backStack)
        assertEquals(emptyList(), returned.forwardStack)
    }

    @Test
    fun routesExposeRegistryIdentityAndDeferredKeysCannotConstructRoutes() {
        assertEquals(ScreenKey.Bootstrap, AppRoute.Bootstrap(BootstrapStep.Welcome).screenKey)
        assertEquals(ScreenKey.PersonalToday, AppRoute.PersonalToday.screenKey)
        assertEquals(ScreenKey.Network, AppRoute.Network.screenKey)
        assertEquals(ScreenKey.Settings, AppRoute.Settings.screenKey)
        assertEquals(
            setOf(ScreenKey.PersonalToday, ScreenKey.Network, ScreenKey.Settings),
            ScreenKey.entries.filter { it.toExecutableRoute() != null }.toSet(),
        )
    }

    @Test
    fun directNavigationAwayFromSettingsNeverAddsSettingsToHistory() {
        val network = NavigationReducer.reduce(NavigationState(AppRoute.PersonalToday), NavigationIntent.Navigate(AppRoute.Network))
        val settings = NavigationReducer.reduce(network, NavigationIntent.Navigate(AppRoute.Settings))
        val today = NavigationReducer.reduce(settings, NavigationIntent.Navigate(AppRoute.PersonalToday))
        assertEquals(AppRoute.PersonalToday, today.current)
        assertEquals(emptyList(), today.backStack)
        assertEquals(emptyList(), today.forwardStack)
    }
}
