package org.radroots.studio.accounts.model

sealed interface AccountsAction {
    data class EditAddDisplayName(
        val value: String,
    ) : AccountsAction

    data class EditAddServerUrl(
        val value: String,
    ) : AccountsAction

    data object SubmitAddAccount : AccountsAction

    data class SelectAccount(
        val accountId: AccountId,
    ) : AccountsAction

    data class LogIn(
        val accountId: AccountId,
    ) : AccountsAction

    data class LogOut(
        val accountId: AccountId,
    ) : AccountsAction

    data class RequestRemoveAccount(
        val accountId: AccountId,
    ) : AccountsAction

    data object CancelRemoveAccount : AccountsAction

    data class ConfirmRemoveAccount(
        val accountId: AccountId,
    ) : AccountsAction

    data object DismissProblem : AccountsAction
}
