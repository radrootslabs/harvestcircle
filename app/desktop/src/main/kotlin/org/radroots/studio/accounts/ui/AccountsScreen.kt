package org.radroots.studio.accounts.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.radroots.studio.accounts.model.Account
import org.radroots.studio.accounts.model.AccountsAction
import org.radroots.studio.accounts.model.AccountsState

@Composable
fun AccountsScreen(
    state: AccountsState,
    onAction: (AccountsAction) -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp)
            .testTag("accounts-screen"),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        BasicText("radroots")
        BasicText("Accounts")

        if (state.accounts.isEmpty()) {
            BasicText(
                text = "No accounts yet.",
                modifier = Modifier.testTag("accounts-empty"),
            )
        } else {
            Row(
                modifier = Modifier.fillMaxSize(),
                horizontalArrangement = Arrangement.spacedBy(24.dp),
            ) {
                LazyColumn(
                    modifier = Modifier
                        .weight(1f)
                        .testTag("accounts-list"),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    items(
                        items = state.accounts,
                        key = { account -> account.id.value },
                    ) { account ->
                        AccountRow(
                            account = account,
                            isSelected = account.id == state.selectedAccountId,
                            onSelect = {
                                onAction(AccountsAction.SelectAccount(account.id))
                            },
                        )
                    }
                }

                state.accounts
                    .firstOrNull { account -> account.id == state.selectedAccountId }
                    ?.let { account ->
                        AccountDetails(
                            account = account,
                            modifier = Modifier.weight(1f),
                        )
                    }
            }
        }
    }
}

@Composable
private fun AccountRow(
    account: Account,
    isSelected: Boolean,
    onSelect: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .semantics { selected = isSelected }
            .testTag("account-row:${account.id.value}")
            .clickable(onClick = onSelect)
            .padding(8.dp),
    ) {
        BasicText(account.displayName)
        BasicText(account.serverUrl)
        if (isSelected) {
            BasicText(
                text = "Selected",
                modifier = Modifier.testTag("account-selected:${account.id.value}"),
            )
        }
    }
}

@Composable
private fun AccountDetails(
    account: Account,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BasicText("Selected account")
        BasicText(account.displayName)
        BasicText(account.serverUrl)
        BasicText("Status: ${account.loginStatus.name}")
    }
}
