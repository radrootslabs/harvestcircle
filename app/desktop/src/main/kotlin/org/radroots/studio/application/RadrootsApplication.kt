package org.radroots.studio.application

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import java.util.UUID
import org.radroots.studio.accounts.model.AccountId
import org.radroots.studio.accounts.model.AccountsState
import org.radroots.studio.accounts.state.AccountIdFactory
import org.radroots.studio.accounts.state.AccountsReducer
import org.radroots.studio.accounts.state.AccountsStore
import org.radroots.studio.accounts.ui.AccountsScreen

@Composable
fun RadrootsApplication(
    store: AccountsStore = remember { createAccountsStore() },
) {
    AccountsScreen(
        state = store.state.value,
        onAction = store::dispatch,
    )
}

internal fun createAccountsStore(
    accountIdFactory: AccountIdFactory = AccountIdFactory {
        AccountId(UUID.randomUUID().toString())
    },
) = AccountsStore(
    initialState = AccountsState(),
    reducer = AccountsReducer(accountIdFactory),
)
