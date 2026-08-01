package org.radroots.studio.accounts.state

import kotlin.test.Test
import kotlin.test.assertEquals
import org.radroots.studio.accounts.model.AccountId
import org.radroots.studio.accounts.model.AccountsAction
import org.radroots.studio.accounts.model.AccountsState

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
}
