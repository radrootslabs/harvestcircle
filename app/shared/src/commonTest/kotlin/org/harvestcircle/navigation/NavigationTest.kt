package org.harvestcircle.navigation

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
        val replaced = NavigationReducer.reduce(back, NavigationIntent.Navigate(AppRoute.Settings(SettingsSection.Project)))
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
        state = NavigationReducer.reduce(state, NavigationIntent.Navigate(AppRoute.Settings(SettingsSection.Appearance)))
        assertEquals(AppRoute.PersonalToday, NavigationReducer.reduce(state, NavigationIntent.ReturnFromSettings).current)
    }
}
