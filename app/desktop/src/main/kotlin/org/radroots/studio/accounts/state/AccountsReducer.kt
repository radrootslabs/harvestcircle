package org.radroots.studio.accounts.state

import org.radroots.studio.accounts.model.Account
import org.radroots.studio.accounts.model.AccountsAction
import org.radroots.studio.accounts.model.AccountsProblem
import org.radroots.studio.accounts.model.AccountsState
import org.radroots.studio.accounts.model.AddAccountDraft
import org.radroots.studio.accounts.model.LoginStatus
import org.radroots.studio.accounts.model.normalizeDisplayName
import org.radroots.studio.accounts.model.normalizeServerUrl
import org.radroots.studio.accounts.model.requireValid

class AccountsReducer(
    private val accountIdFactory: AccountIdFactory,
) {
    fun reduce(
        state: AccountsState,
        action: AccountsAction,
    ): AccountsState {
        state.requireValid()
        val nextState = when (action) {
            is AccountsAction.EditAddDisplayName -> state.copy(
                addDraft = state.addDraft.copy(displayName = action.value),
            )
            is AccountsAction.EditAddServerUrl -> state.copy(
                addDraft = state.addDraft.copy(serverUrl = action.value),
            )
            AccountsAction.SubmitAddAccount -> submitAddAccount(state)
            AccountsAction.DismissProblem -> state.copy(problem = null)
            else -> state
        }
        return nextState.requireValid()
    }

    private fun submitAddAccount(state: AccountsState): AccountsState {
        val displayName = normalizeDisplayName(state.addDraft.displayName)
            ?: return state.copy(problem = AccountsProblem.BlankDisplayName)
        val serverUrl = normalizeServerUrl(state.addDraft.serverUrl)
            ?: return state.copy(problem = AccountsProblem.InvalidServerUrl)
        val accountId = accountIdFactory.nextId()
        if (accountId.value.isBlank() || accountId.value != accountId.value.trim()) {
            return state.copy(problem = AccountsProblem.InvalidGeneratedAccountId)
        }
        if (state.accounts.any { it.id == accountId }) {
            return state.copy(problem = AccountsProblem.DuplicateAccountId(accountId))
        }

        val account = Account(
            id = accountId,
            displayName = displayName,
            serverUrl = serverUrl,
            loginStatus = LoginStatus.LoggedOut,
        )
        return state.copy(
            accounts = state.accounts + account,
            selectedAccountId = accountId,
            addDraft = AddAccountDraft(),
            pendingRemovalAccountId = null,
            problem = null,
        )
    }
}
