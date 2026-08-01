package org.radroots.studio.accounts

import org.radroots.studio.accounts.model.Account
import org.radroots.studio.accounts.model.AccountId
import org.radroots.studio.accounts.model.AccountsState
import org.radroots.studio.accounts.model.LoginStatus

internal fun testAccount(
    id: String = "account-1",
    displayName: String = "Farm Account",
    serverUrl: String = "https://farm.example.test",
    loginStatus: LoginStatus = LoginStatus.LoggedOut,
) = Account(
    id = AccountId(id),
    displayName = displayName,
    serverUrl = serverUrl,
    loginStatus = loginStatus,
)

internal fun testAccountsState(
    vararg accounts: Account,
    selectedAccountId: AccountId? = accounts.firstOrNull()?.id,
) = AccountsState(
    accounts = accounts.toList(),
    selectedAccountId = selectedAccountId,
)
