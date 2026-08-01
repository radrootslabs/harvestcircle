package org.radroots.studio.accounts.state

import java.util.ArrayDeque
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import org.radroots.studio.accounts.model.AccountId
import org.radroots.studio.accounts.model.AccountsAction
import org.radroots.studio.accounts.model.AccountsProblem
import org.radroots.studio.accounts.model.AccountsState
import org.radroots.studio.accounts.model.LoginStatus
import org.radroots.studio.accounts.testAccount
import org.radroots.studio.accounts.testAccountsState

class AccountsReducerTest {
    @Test
    fun draftEditsChangeOnlyTheRequestedField() {
        val reducer = reducerWithIds("unused")

        val named = reducer.reduce(
            AccountsState(),
            AccountsAction.EditAddDisplayName("Farm Account"),
        )
        val addressed = reducer.reduce(
            named,
            AccountsAction.EditAddServerUrl("https://farm.example.test"),
        )

        assertEquals("Farm Account", addressed.addDraft.displayName)
        assertEquals("https://farm.example.test", addressed.addDraft.serverUrl)
        assertEquals(emptyList(), addressed.accounts)
    }

    @Test
    fun validSubmitNormalizesAddsAndSelectsLoggedOutAccount() {
        val factory = RecordingIdFactory("account-1")
        val reducer = AccountsReducer(factory)
        val state = draftState(" Farm Account ", " HTTPS://FARM.EXAMPLE.TEST/a/../b ")

        val result = reducer.reduce(state, AccountsAction.SubmitAddAccount)

        assertEquals(1, factory.callCount)
        assertEquals(AccountId("account-1"), result.selectedAccountId)
        assertEquals("Farm Account", result.accounts.single().displayName)
        assertEquals("https://farm.example.test/b", result.accounts.single().serverUrl)
        assertEquals(LoginStatus.LoggedOut, result.accounts.single().loginStatus)
        assertEquals("", result.addDraft.displayName)
        assertEquals("", result.addDraft.serverUrl)
        assertEquals(null, result.problem)
    }

    @Test
    fun invalidDraftDoesNotRequestAnIdOrMutateAccounts() {
        val factory = RecordingIdFactory("unused")
        val reducer = AccountsReducer(factory)
        val existing = testAccount()
        val base = testAccountsState(existing)

        val blankName = reducer.reduce(
            base.copy(
                addDraft = base.addDraft.copy(serverUrl = "https://farm.example.test"),
            ),
            AccountsAction.SubmitAddAccount,
        )
        val invalidUrl = reducer.reduce(
            base.copy(
                addDraft = base.addDraft.copy(displayName = "Second Account", serverUrl = "ftp://host"),
            ),
            AccountsAction.SubmitAddAccount,
        )

        assertEquals(0, factory.callCount)
        assertEquals(base.accounts, blankName.accounts)
        assertEquals(base.selectedAccountId, blankName.selectedAccountId)
        assertIs<AccountsProblem.BlankDisplayName>(blankName.problem)
        assertEquals(base.accounts, invalidUrl.accounts)
        assertIs<AccountsProblem.InvalidServerUrl>(invalidUrl.problem)
    }

    @Test
    fun invalidOrDuplicateGeneratedIdIsAStableFailure() {
        val existing = testAccount()
        val base = testAccountsState(existing).copy(
            addDraft = draftState("Second Account", "https://second.example.test").addDraft,
        )

        val blank = AccountsReducer(RecordingIdFactory(" ")).reduce(
            base,
            AccountsAction.SubmitAddAccount,
        )
        val duplicate = AccountsReducer(RecordingIdFactory(existing.id.value)).reduce(
            base,
            AccountsAction.SubmitAddAccount,
        )

        assertEquals(base.accounts, blank.accounts)
        assertIs<AccountsProblem.InvalidGeneratedAccountId>(blank.problem)
        assertEquals(base.accounts, duplicate.accounts)
        assertEquals(AccountsProblem.DuplicateAccountId(existing.id), duplicate.problem)
    }

    @Test
    fun dismissProblemClearsOnlyTheProblem() {
        val state = testAccountsState(testAccount()).copy(
            problem = AccountsProblem.InvalidServerUrl,
        )

        val result = reducerWithIds("unused").reduce(
            state,
            AccountsAction.DismissProblem,
        )

        assertEquals(state.copy(problem = null), result)
    }

    private fun draftState(
        displayName: String,
        serverUrl: String,
    ) = AccountsState(
        addDraft = org.radroots.studio.accounts.model.AddAccountDraft(
            displayName = displayName,
            serverUrl = serverUrl,
        ),
    )

    private fun reducerWithIds(vararg ids: String) =
        AccountsReducer(RecordingIdFactory(*ids))
}

private class RecordingIdFactory(
    vararg ids: String,
) : AccountIdFactory {
    private val ids = ArrayDeque(ids.map(::AccountId))

    var callCount = 0
        private set

    override fun nextId(): AccountId {
        callCount += 1
        return ids.removeFirst()
    }
}
