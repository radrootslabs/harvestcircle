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

    @Test
    fun selectionTargetsAnExplicitExistingAccount() {
        val first = testAccount()
        val second = testAccount(id = "account-2", displayName = "Second Account")
        val base = testAccountsState(first, second)
        val reducer = reducerWithIds("unused")

        val selected = reducer.reduce(
            base.copy(problem = AccountsProblem.InvalidServerUrl),
            AccountsAction.SelectAccount(second.id),
        )
        val missingId = AccountId("missing")
        val missing = reducer.reduce(
            selected,
            AccountsAction.SelectAccount(missingId),
        )

        assertEquals(second.id, selected.selectedAccountId)
        assertEquals(null, selected.problem)
        assertEquals(second.id, missing.selectedAccountId)
        assertEquals(AccountsProblem.AccountNotFound(missingId), missing.problem)
    }

    @Test
    fun loginAndLogoutUpdateOnlyTheExplicitAccount() {
        val first = testAccount()
        val second = testAccount(id = "account-2", displayName = "Second Account")
        val base = testAccountsState(first, second)
        val reducer = reducerWithIds("unused")

        val loggedIn = reducer.reduce(base, AccountsAction.LogIn(second.id))
        val idempotentLogin = reducer.reduce(loggedIn, AccountsAction.LogIn(second.id))
        val loggedOut = reducer.reduce(idempotentLogin, AccountsAction.LogOut(second.id))
        val idempotentLogout = reducer.reduce(loggedOut, AccountsAction.LogOut(second.id))

        assertEquals(LoginStatus.LoggedOut, loggedIn.accounts[0].loginStatus)
        assertEquals(LoginStatus.LoggedIn, loggedIn.accounts[1].loginStatus)
        assertEquals(loggedIn, idempotentLogin)
        assertEquals(LoginStatus.LoggedOut, loggedOut.accounts[1].loginStatus)
        assertEquals(loggedOut, idempotentLogout)
        assertEquals(first.id, loggedOut.selectedAccountId)
    }

    @Test
    fun loginAndLogoutRejectMissingTargets() {
        val base = testAccountsState(testAccount())
        val missingId = AccountId("missing")
        val reducer = reducerWithIds("unused")

        listOf(
            reducer.reduce(base, AccountsAction.LogIn(missingId)),
            reducer.reduce(base, AccountsAction.LogOut(missingId)),
        ).forEach { result ->
            assertEquals(base.accounts, result.accounts)
            assertEquals(base.selectedAccountId, result.selectedAccountId)
            assertEquals(AccountsProblem.AccountNotFound(missingId), result.problem)
        }
    }

    @Test
    fun removalRequestTargetsAnExistingAccountAndCanBeCancelled() {
        val first = testAccount()
        val second = testAccount(id = "account-2", displayName = "Second Account")
        val base = testAccountsState(first, second)
        val reducer = reducerWithIds("unused")

        val firstRequest = reducer.reduce(
            base,
            AccountsAction.RequestRemoveAccount(first.id),
        )
        val replacedRequest = reducer.reduce(
            firstRequest,
            AccountsAction.RequestRemoveAccount(second.id),
        )
        val missingId = AccountId("missing")
        val missingRequest = reducer.reduce(
            replacedRequest,
            AccountsAction.RequestRemoveAccount(missingId),
        )
        val cancelled = reducer.reduce(
            replacedRequest.copy(problem = AccountsProblem.InvalidServerUrl),
            AccountsAction.CancelRemoveAccount,
        )

        assertEquals(first.id, firstRequest.pendingRemovalAccountId)
        assertEquals(second.id, replacedRequest.pendingRemovalAccountId)
        assertEquals(second.id, missingRequest.pendingRemovalAccountId)
        assertEquals(AccountsProblem.AccountNotFound(missingId), missingRequest.problem)
        assertEquals(null, cancelled.pendingRemovalAccountId)
        assertEquals(null, cancelled.problem)
        assertEquals(base.accounts, cancelled.accounts)
    }

    @Test
    fun confirmationRequiresThePendingExplicitTarget() {
        val first = testAccount()
        val second = testAccount(id = "account-2", displayName = "Second Account")
        val reducer = reducerWithIds("unused")
        val base = testAccountsState(first, second)

        val withoutRequest = reducer.reduce(
            base,
            AccountsAction.ConfirmRemoveAccount(first.id),
        )
        val requested = reducer.reduce(
            base,
            AccountsAction.RequestRemoveAccount(first.id),
        )
        val mismatch = reducer.reduce(
            requested,
            AccountsAction.ConfirmRemoveAccount(second.id),
        )

        assertEquals(
            AccountsProblem.RemovalTargetMismatch(null, first.id),
            withoutRequest.problem,
        )
        assertEquals(base.accounts, withoutRequest.accounts)
        assertEquals(
            AccountsProblem.RemovalTargetMismatch(first.id, second.id),
            mismatch.problem,
        )
        assertEquals(requested.accounts, mismatch.accounts)
        assertEquals(first.id, mismatch.pendingRemovalAccountId)
    }

    @Test
    fun removingSelectedAccountChoosesTheDeterministicNeighbor() {
        val first = testAccount()
        val second = testAccount(id = "account-2", displayName = "Second Account")
        val third = testAccount(id = "account-3", displayName = "Third Account")
        val reducer = reducerWithIds("unused")

        val removedFirst = reducer.removeSelected(testAccountsState(first, second, third), first.id)
        val removedMiddle = reducer.removeSelected(
            testAccountsState(first, second, third, selectedAccountId = second.id),
            second.id,
        )
        val removedFinal = reducer.removeSelected(
            testAccountsState(first, second, third, selectedAccountId = third.id),
            third.id,
        )

        assertEquals(listOf(second, third), removedFirst.accounts)
        assertEquals(second.id, removedFirst.selectedAccountId)
        assertEquals(listOf(first, third), removedMiddle.accounts)
        assertEquals(third.id, removedMiddle.selectedAccountId)
        assertEquals(listOf(first, second), removedFinal.accounts)
        assertEquals(second.id, removedFinal.selectedAccountId)
    }

    @Test
    fun removingUnselectedOrFinalAccountPreservesValidSelection() {
        val first = testAccount()
        val second = testAccount(id = "account-2", displayName = "Second Account")
        val reducer = reducerWithIds("unused")

        val unselected = reducer.removeSelected(testAccountsState(first, second), second.id)
        val finalAccount = reducer.removeSelected(testAccountsState(first), first.id)

        assertEquals(listOf(first), unselected.accounts)
        assertEquals(first.id, unselected.selectedAccountId)
        assertEquals(emptyList(), finalAccount.accounts)
        assertEquals(null, finalAccount.selectedAccountId)
        assertEquals(null, finalAccount.pendingRemovalAccountId)
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

    private fun AccountsReducer.removeSelected(
        state: AccountsState,
        accountId: AccountId,
    ): AccountsState {
        val requested = reduce(state, AccountsAction.RequestRemoveAccount(accountId))
        return reduce(requested, AccountsAction.ConfirmRemoveAccount(accountId))
    }
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
