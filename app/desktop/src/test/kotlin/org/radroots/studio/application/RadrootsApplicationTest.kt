package org.radroots.studio.application

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import org.radroots.studio.accounts.model.AccountsAction

class RadrootsApplicationTest {
    @OptIn(ExperimentalTestApi::class)
    @Test
    fun composeUiTestRuntimeRendersContent() = runComposeUiTest {
        setContent {
            RadrootsApplication()
        }

        onNodeWithText("radroots").assertIsDisplayed()
    }

    @Test
    fun applicationStoresStartEmptyAndGenerateIndependentIds() {
        val first = createAccountsStore()
        val second = createAccountsStore()

        listOf(first, second).forEach { store ->
            assertEquals(emptyList(), store.state.value.accounts)
            store.dispatch(AccountsAction.EditAddDisplayName("Farm Account"))
            store.dispatch(
                AccountsAction.EditAddServerUrl("https://farm.example.test"),
            )
            store.dispatch(AccountsAction.SubmitAddAccount)
        }

        assertNotEquals(
            first.state.value.accounts.single().id,
            second.state.value.accounts.single().id,
        )
    }
}
