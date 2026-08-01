package org.radroots.studio.accounts.state

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import org.radroots.studio.accounts.model.AccountId
import org.radroots.studio.accounts.model.AccountsAction
import org.radroots.studio.accounts.model.AccountsState
import org.radroots.studio.accounts.testAccount

class AccountsStoreTest {
    @Test
    fun storeExposesInitialStateAndDispatchesThroughReducer() {
        val store = AccountsStore(
            initialState = AccountsState(),
            reducer = AccountsReducer { AccountId("account-1") },
        )

        assertEquals(AccountsState(), store.state.value)

        store.dispatch(AccountsAction.EditAddDisplayName("Farm Account"))
        store.dispatch(AccountsAction.EditAddServerUrl("https://farm.example.test"))
        store.dispatch(AccountsAction.SubmitAddAccount)

        assertEquals(AccountId("account-1"), store.state.value.selectedAccountId)
        assertEquals("Farm Account", store.state.value.accounts.single().displayName)
    }

    @Test
    fun observedStateTracksSequentialDispatches() {
        val store = AccountsStore(
            initialState = AccountsState(),
            reducer = AccountsReducer { AccountId("account-1") },
        )
        val observedState = store.state

        store.dispatch(AccountsAction.EditAddDisplayName("Farm Account"))
        assertEquals("Farm Account", observedState.value.addDraft.displayName)
        store.dispatch(AccountsAction.EditAddServerUrl("https://farm.example.test"))
        store.dispatch(AccountsAction.SubmitAddAccount)
        store.dispatch(AccountsAction.LogIn(AccountId("account-1")))

        assertEquals(store.state.value, observedState.value)
        assertEquals(1, observedState.value.accounts.size)
    }

    @Test
    fun invalidInitialStateFailsAtConstruction() {
        assertFailsWith<IllegalArgumentException> {
            AccountsStore(
                initialState = AccountsState(accounts = listOf(testAccount())),
                reducer = AccountsReducer { AccountId("unused") },
            )
        }
    }

    @Test
    fun storeInstancesNeverShareState() {
        val first = AccountsStore(
            initialState = AccountsState(),
            reducer = AccountsReducer { AccountId("first") },
        )
        val second = AccountsStore(
            initialState = AccountsState(),
            reducer = AccountsReducer { AccountId("second") },
        )

        first.dispatch(AccountsAction.EditAddDisplayName("First Account"))
        first.dispatch(AccountsAction.EditAddServerUrl("https://first.example.test"))
        first.dispatch(AccountsAction.SubmitAddAccount)

        assertEquals(listOf(AccountId("first")), first.state.value.accounts.map { it.id })
        assertEquals(AccountsState(), second.state.value)
    }
}
