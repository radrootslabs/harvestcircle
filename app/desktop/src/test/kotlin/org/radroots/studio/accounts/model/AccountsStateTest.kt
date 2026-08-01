package org.radroots.studio.accounts.model

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
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

    @Test
    fun validStatePassesInvariantValidation() {
        val account = validAccount()

        assertEquals(
            AccountsState(
                accounts = listOf(account),
                selectedAccountId = account.id,
            ),
            AccountsState(
                accounts = listOf(account),
                selectedAccountId = account.id,
            ).requireValid(),
        )
    }

    @Test
    fun serverUrlNormalizationIsStableAndBounded() {
        assertEquals(
            "https://farm.example.test/b?mode=local",
            normalizeServerUrl(" HTTPS://FARM.EXAMPLE.TEST/a/../b?mode=local "),
        )
        assertEquals("http://localhost:8080", normalizeServerUrl("http://localhost:8080"))
        assertNull(normalizeServerUrl("ftp://farm.example.test"))
        assertNull(normalizeServerUrl("https:///missing-host"))
        assertNull(normalizeServerUrl("https://user@farm.example.test"))
        assertNull(normalizeServerUrl("https://farm.example.test/#fragment"))
        assertNull(normalizeServerUrl("not a url"))
    }

    @Test
    fun invalidAccountValuesAreRejected() {
        invalidAccounts().forEach { account ->
            assertFailsWith<IllegalArgumentException> {
                AccountsState(
                    accounts = listOf(account),
                    selectedAccountId = account.id,
                ).requireValid()
            }
        }
    }

    @Test
    fun duplicateIdsAreRejected() {
        val account = validAccount()

        assertFailsWith<IllegalArgumentException> {
            AccountsState(
                accounts = listOf(account, account.copy(displayName = "Second Account")),
                selectedAccountId = account.id,
            ).requireValid()
        }
    }

    @Test
    fun danglingSelectionAndRemovalTargetsAreRejected() {
        val account = validAccount()
        val missingId = AccountId("missing")

        listOf(
            AccountsState(accounts = listOf(account)),
            AccountsState(
                accounts = listOf(account),
                selectedAccountId = missingId,
            ),
            AccountsState(selectedAccountId = missingId),
            AccountsState(
                accounts = listOf(account),
                selectedAccountId = account.id,
                pendingRemovalAccountId = missingId,
            ),
        ).forEach { state ->
            assertFailsWith<IllegalArgumentException> {
                state.requireValid()
            }
        }
    }

    private fun validAccount() = Account(
        id = AccountId("account-1"),
        displayName = "Farm Account",
        serverUrl = "https://farm.example.test",
        loginStatus = LoginStatus.LoggedOut,
    )

    private fun invalidAccounts() = listOf(
        validAccount().copy(id = AccountId("")),
        validAccount().copy(id = AccountId(" account-1")),
        validAccount().copy(displayName = ""),
        validAccount().copy(displayName = " Farm Account"),
        validAccount().copy(serverUrl = "ftp://farm.example.test"),
        validAccount().copy(serverUrl = "HTTPS://FARM.EXAMPLE.TEST"),
    )
}
