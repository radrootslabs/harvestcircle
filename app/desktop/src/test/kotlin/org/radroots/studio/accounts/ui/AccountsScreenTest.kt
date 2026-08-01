package org.radroots.studio.accounts.ui

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test
import kotlin.test.assertEquals
import org.radroots.studio.accounts.model.AccountsAction
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
}
