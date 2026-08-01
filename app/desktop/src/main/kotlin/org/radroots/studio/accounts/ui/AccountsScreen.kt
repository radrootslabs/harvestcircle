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
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.radroots.studio.accounts.model.Account
import org.radroots.studio.accounts.model.AccountsAction
import org.radroots.studio.accounts.model.AccountsState
import org.radroots.studio.accounts.model.AccountsProblem
import org.radroots.studio.accounts.model.LoginStatus

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
        AddAccountForm(state = state, onAction = onAction)

        state.problem?.let { problem ->
            BasicText(
                text = problemMessage(problem),
                modifier = Modifier.testTag("accounts-problem"),
            )
        }

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
                            onLogIn = {
                                onAction(AccountsAction.LogIn(account.id))
                            },
                            onLogOut = {
                                onAction(AccountsAction.LogOut(account.id))
                            },
                            isPendingRemoval = account.id == state.pendingRemovalAccountId,
                            onRequestRemoval = {
                                onAction(AccountsAction.RequestRemoveAccount(account.id))
                            },
                            onConfirmRemoval = {
                                onAction(AccountsAction.ConfirmRemoveAccount(account.id))
                            },
                            onCancelRemoval = {
                                onAction(AccountsAction.CancelRemoveAccount)
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
private fun AddAccountForm(
    state: AccountsState,
    onAction: (AccountsAction) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        BasicText("Add account")
        BasicTextField(
            value = state.addDraft.displayName,
            onValueChange = { value ->
                onAction(AccountsAction.EditAddDisplayName(value))
            },
            modifier = Modifier
                .fillMaxWidth()
                .testTag("add-display-name")
                .padding(8.dp),
            decorationBox = { innerTextField ->
                if (state.addDraft.displayName.isEmpty()) {
                    BasicText("Display name")
                }
                innerTextField()
            },
        )
        BasicTextField(
            value = state.addDraft.serverUrl,
            onValueChange = { value ->
                onAction(AccountsAction.EditAddServerUrl(value))
            },
            modifier = Modifier
                .fillMaxWidth()
                .testTag("add-server-url")
                .padding(8.dp),
            decorationBox = { innerTextField ->
                if (state.addDraft.serverUrl.isEmpty()) {
                    BasicText("Server URL")
                }
                innerTextField()
            },
        )
        TextAction(
            text = "Add Account",
            testTag = "add-submit",
            onClick = { onAction(AccountsAction.SubmitAddAccount) },
        )
    }
}

@Composable
private fun TextAction(
    text: String,
    testTag: String,
    onClick: () -> Unit,
) {
    BasicText(
        text = text,
        modifier = Modifier
            .semantics { role = Role.Button }
            .testTag(testTag)
            .clickable(onClick = onClick)
            .padding(8.dp),
    )
}

private fun problemMessage(problem: AccountsProblem): String = when (problem) {
    AccountsProblem.BlankDisplayName -> "Display name is required."
    AccountsProblem.InvalidServerUrl -> "Enter a valid HTTP or HTTPS server URL."
    AccountsProblem.InvalidGeneratedAccountId -> "Could not create an account ID."
    is AccountsProblem.DuplicateAccountId -> "That account ID already exists."
    is AccountsProblem.AccountNotFound -> "That account no longer exists."
    is AccountsProblem.RemovalTargetMismatch -> "The removal target changed."
}

@Composable
private fun AccountRow(
    account: Account,
    isSelected: Boolean,
    onSelect: () -> Unit,
    onLogIn: () -> Unit,
    onLogOut: () -> Unit,
    isPendingRemoval: Boolean,
    onRequestRemoval: () -> Unit,
    onConfirmRemoval: () -> Unit,
    onCancelRemoval: () -> Unit,
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
        when (account.loginStatus) {
            LoginStatus.LoggedOut -> TextAction(
                text = "Login",
                testTag = "account-login:${account.id.value}",
                onClick = onLogIn,
            )
            LoginStatus.LoggedIn -> TextAction(
                text = "Logout",
                testTag = "account-logout:${account.id.value}",
                onClick = onLogOut,
            )
        }
        if (isPendingRemoval) {
            BasicText("Remove this account?")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextAction(
                    text = "Confirm",
                    testTag = "remove-confirm",
                    onClick = onConfirmRemoval,
                )
                TextAction(
                    text = "Cancel",
                    testTag = "remove-cancel",
                    onClick = onCancelRemoval,
                )
            }
        } else {
            TextAction(
                text = "Remove",
                testTag = "account-remove:${account.id.value}",
                onClick = onRequestRemoval,
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
