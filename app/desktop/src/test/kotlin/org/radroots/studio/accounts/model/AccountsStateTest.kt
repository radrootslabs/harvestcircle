package org.radroots.studio.accounts.model

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import org.radroots.studio.accounts.testAccount

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
        val account = testAccount()

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
        val account = testAccount()

        assertFailsWith<IllegalArgumentException> {
            AccountsState(
                accounts = listOf(account, account.copy(displayName = "Second Account")),
                selectedAccountId = account.id,
            ).requireValid()
        }
    }

    @Test
    fun danglingSelectionAndRemovalTargetsAreRejected() {
        val account = testAccount()
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

    private fun invalidAccounts() = listOf(
        testAccount().copy(id = AccountId("")),
        testAccount().copy(id = AccountId(" account-1")),
        testAccount().copy(displayName = ""),
        testAccount().copy(displayName = " Farm Account"),
        testAccount().copy(serverUrl = "ftp://farm.example.test"),
        testAccount().copy(serverUrl = "HTTPS://FARM.EXAMPLE.TEST"),
    )
}
