package org.radroots.studio.accounts.model

data class AddAccountDraft(
    val displayName: String = "",
    val serverUrl: String = "",
)

data class AccountsState(
    val accounts: List<Account> = emptyList(),
    val selectedAccountId: AccountId? = null,
    val addDraft: AddAccountDraft = AddAccountDraft(),
    val pendingRemovalAccountId: AccountId? = null,
    val problem: AccountsProblem? = null,
)
