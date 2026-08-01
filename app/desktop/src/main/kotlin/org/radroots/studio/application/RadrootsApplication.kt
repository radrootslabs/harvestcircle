package org.radroots.studio.application

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import java.util.UUID
import org.radroots.studio.accounts.model.AccountId
import org.radroots.studio.accounts.model.AccountsState
import org.radroots.studio.accounts.state.AccountIdFactory
import org.radroots.studio.accounts.state.AccountsReducer
import org.radroots.studio.accounts.state.AccountsStore

@Composable
fun RadrootsApplication(
    store: AccountsStore = remember { createAccountsStore() },
) {
    key(store.state.value) {
        Box(
            modifier = Modifier.fillMaxSize(),
            contentAlignment = Alignment.Center,
        ) {
            BasicText("radroots")
        }
    }
}

internal fun createAccountsStore(
    accountIdFactory: AccountIdFactory = AccountIdFactory {
        AccountId(UUID.randomUUID().toString())
    },
) = AccountsStore(
    initialState = AccountsState(),
    reducer = AccountsReducer(accountIdFactory),
)
