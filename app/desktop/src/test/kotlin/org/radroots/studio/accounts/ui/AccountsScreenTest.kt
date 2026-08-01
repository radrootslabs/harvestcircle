package org.radroots.studio.accounts.ui

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlin.test.Test
import kotlin.test.assertEquals
import org.radroots.studio.accounts.model.AccountsAction
import org.radroots.studio.accounts.model.AccountId
import org.radroots.studio.accounts.model.AccountsProblem
import org.radroots.studio.accounts.model.AccountsState
import org.radroots.studio.accounts.state.AccountsReducer
import org.radroots.studio.accounts.state.AccountsStore
import org.radroots.studio.accounts.testAccount
import org.radroots.studio.accounts.testAccountsState

@OptIn(ExperimentalTestApi::class)
class AccountsScreenTest {
    @Test
    fun emptyStateIsVisible() = runComposeUiTest {
        setContent {
            AccountsScreen(
                state = testAccountsState(),
                onAction = {},
            )
        }

        onNodeWithTag("accounts-screen").assertIsDisplayed()
        onNodeWithTag("accounts-empty").assertIsDisplayed()
        onNodeWithText("No accounts yet.").assertIsDisplayed()
    }

    @Test
    fun accountListShowsSelectionAndEmitsExplicitSelection() = runComposeUiTest {
        val first = testAccount()
        val second = testAccount(id = "account-2", displayName = "Second Account")
        val actions = mutableListOf<AccountsAction>()
        setContent {
            AccountsScreen(
                state = testAccountsState(first, second),
                onAction = actions::add,
            )
        }

        onNodeWithTag("accounts-list").assertIsDisplayed()
        onNodeWithTag("account-row:${first.id.value}").assertIsSelected()
        onNodeWithTag(
            testTag = "account-selected:${first.id.value}",
            useUnmergedTree = true,
        ).assertIsDisplayed()
        onNodeWithTag("account-row:${second.id.value}").performClick()

        assertEquals(
            listOf<AccountsAction>(AccountsAction.SelectAccount(second.id)),
            actions,
        )
    }

    @Test
    fun addFormEmitsTypedDraftAndSubmitActions() = runComposeUiTest {
        val actions = mutableListOf<AccountsAction>()
        setContent {
            var state by remember { mutableStateOf(AccountsState()) }
            AccountsScreen(
                state = state,
                onAction = { action ->
                    actions.add(action)
                    state = when (action) {
                        is AccountsAction.EditAddDisplayName -> state.copy(
                            addDraft = state.addDraft.copy(displayName = action.value),
                        )
                        is AccountsAction.EditAddServerUrl -> state.copy(
                            addDraft = state.addDraft.copy(serverUrl = action.value),
                        )
                        else -> state
                    }
                },
            )
        }

        onNodeWithTag("add-display-name").performTextInput("Farm Account")
        onNodeWithTag("add-server-url").performTextInput("https://farm.example.test")
        onNodeWithTag("add-submit").performClick()

        assertEquals(
            listOf(
                AccountsAction.EditAddDisplayName("Farm Account"),
                AccountsAction.EditAddServerUrl("https://farm.example.test"),
                AccountsAction.SubmitAddAccount,
            ),
            actions,
        )
    }

    @Test
    fun problemIsDisplayedWithoutChangingTheProvidedState() = runComposeUiTest {
        setContent {
            AccountsScreen(
                state = AccountsState(problem = AccountsProblem.InvalidServerUrl),
                onAction = {},
            )
        }

        onNodeWithTag("accounts-problem").assertIsDisplayed()
        onNodeWithText("Enter a valid HTTP or HTTPS server URL.").assertIsDisplayed()
    }

    @Test
    fun integratedFormAddsAndSelectsAnAccount() = runComposeUiTest {
        val store = AccountsStore(
            initialState = AccountsState(),
            reducer = AccountsReducer { AccountId("account-1") },
        )
        setContent {
            AccountsScreen(
                state = store.state.value,
                onAction = store::dispatch,
            )
        }

        onNodeWithTag("add-display-name").performTextInput("Farm Account")
        onNodeWithTag("add-server-url").performTextInput("https://farm.example.test")
        onNodeWithTag("add-submit").performClick()

        onNodeWithTag("account-row:account-1").assertIsDisplayed()
        onNodeWithTag("account-row:account-1").assertIsSelected()
    }
}
