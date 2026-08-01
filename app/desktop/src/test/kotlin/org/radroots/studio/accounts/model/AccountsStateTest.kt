package org.radroots.studio.accounts.model

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class AccountsStateTest {
    @Test
    fun defaultStateIsEmpty() {
        val state = AccountsState()

        assertEquals(emptyList(), state.accounts)
        assertNull(state.selectedAccountId)
        assertEquals(AddAccountDraft(), state.addDraft)
        assertNull(state.pendingRemovalAccountId)
        assertNull(state.problem)
    }

    @Test
    fun accountContainsOnlyTheApprovedProofFields() {
        val account = Account(
            id = AccountId("account-1"),
            displayName = "Farm Account",
            serverUrl = "https://farm.example.test",
            loginStatus = LoginStatus.LoggedOut,
        )

        assertEquals("account-1", account.id.value)
        assertEquals("Farm Account", account.displayName)
        assertEquals("https://farm.example.test", account.serverUrl)
        assertEquals(LoginStatus.LoggedOut, account.loginStatus)
    }
}
