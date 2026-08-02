package org.radroots.studio.accounts.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import org.radroots.studio.ffi.SessionStateDto

@OptIn(ExperimentalTestApi::class)
class StudioScreenTest {
    @Test
    fun inactiveScreenGeneratesAndImportsMaskedSecretInput() = runComposeUiTest {
        var importDraft by mutableStateOf("")
        var generateCalls = 0
        var importCalls = 0
        setContent {
            StudioScreen(
                model = emptyUiModel(importDraft = importDraft),
                actions = StudioUiActions(
                    editImportDraft = { importDraft = it },
                    generateAccount = { generateCalls += 1 },
                    importSecretKey = { importCalls += 1 },
                ),
            )
        }

        onNodeWithTag("accounts-screen").assertIsDisplayed()
        onNodeWithText("radroots").assertIsDisplayed()
        onNodeWithTag("generate-key").performClick()
        onNodeWithTag("import-nsec-input").performTextInput("nsec1secret")
        onNodeWithTag("import-key").performClick()

        assertEquals(1, generateCalls)
        assertEquals(1, importCalls)
        assertEquals("nsec1secret", importDraft)
        assertTrue(onNodeWithTag("import-nsec-input").fetchSemanticsNode().config.any {
            it.key.name == "Password" && it.value == Unit
        })
    }

    @Test
    fun inactiveScreenShowsSafeFailureAndNoGenericFields() = runComposeUiTest {
        setContent {
            StudioScreen(
                model = emptyUiModel(problem = "The secret key is invalid."),
                actions = StudioUiActions(),
            )
        }

        onNodeWithText("The secret key is invalid.").assertIsDisplayed()
        onNodeWithTag("accounts-empty").assertIsDisplayed()
    }

    @Test
    fun generatedKeyBackupCopiesAndClearsOnlyAfterAcknowledgement() = runComposeUiTest {
        var backup: GeneratedKeyBackupUiModel? by mutableStateOf(
            GeneratedKeyBackupUiModel("npub1generated", "nsec1generated"),
        )
        var copied: String? = null
        setContent {
            StudioScreen(
                model = emptyUiModel().copy(generatedKeyBackup = backup),
                actions = StudioUiActions(
                    copyText = { copied = it },
                    acknowledgeGeneratedKeyBackup = { backup = null },
                ),
            )
        }

        onNodeWithTag("generated-key-backup").assertIsDisplayed()
        onNodeWithTag("generated-nsec").assertIsDisplayed()
        onNodeWithTag("copy-generated-key").performClick()
        assertEquals("nsec1generated", copied)

        onNodeWithTag("acknowledge-key-backup").performClick()
        onAllNodesWithTag("generated-key-backup").assertCountEquals(0)
        onAllNodesWithTag("generated-nsec").assertCountEquals(0)
    }

    @Test
    fun savedAccountsSelectActivateAndRequireRemovalConfirmation() = runComposeUiTest {
        val first = accountUi("11".repeat(32), selected = true)
        val second = accountUi("22".repeat(32), selected = false)
        var pendingRemoval: String? by mutableStateOf(null)
        val selected = mutableListOf<String>()
        val activated = mutableListOf<String>()
        var confirmations = 0
        setContent {
            StudioScreen(
                model = emptyUiModel().copy(
                    accounts = listOf(first, second),
                    pendingRemovalPublicKeyHex = pendingRemoval,
                ),
                actions = StudioUiActions(
                    selectAccount = selected::add,
                    activateAccount = activated::add,
                    requestAccountRemoval = { pendingRemoval = it },
                    cancelAccountRemoval = { pendingRemoval = null },
                    confirmAccountRemoval = { confirmations += 1 },
                ),
            )
        }

        onNodeWithTag("saved-account-list").assertIsDisplayed()
        onNodeWithTag("account-row:${first.publicKeyHex}").assertIsSelected()
        onNodeWithTag("account-row:${second.publicKeyHex}").performClick()
        onNodeWithTag("activate-account:${second.publicKeyHex}", useUnmergedTree = true).performClick()
        assertEquals(listOf(second.publicKeyHex), selected)
        assertEquals(listOf(second.publicKeyHex), activated)

        onNodeWithTag("remove-account:${second.publicKeyHex}", useUnmergedTree = true).performClick()
        onNodeWithTag("remove-cancel", useUnmergedTree = true).performClick()
        assertEquals(null, pendingRemoval)
        onNodeWithTag("remove-account:${second.publicKeyHex}", useUnmergedTree = true).performClick()
        onNodeWithTag("remove-confirm", useUnmergedTree = true).performClick()
        assertEquals(1, confirmations)
    }
}

private fun emptyUiModel(
    importDraft: String = "",
    problem: String? = null,
) = StudioUiModel(
    accounts = emptyList(),
    activeAccount = null,
    configuredRelays = emptyList(),
    importDraft = importDraft,
    generatedKeyBackup = null,
    pendingRemovalPublicKeyHex = null,
    session = SessionStateDto.SIGNED_OUT,
    busy = false,
    problem = problem,
)

private fun accountUi(publicKeyHex: String, selected: Boolean) = AccountUiModel(
    publicKeyHex = publicKeyHex,
    npub = "npub1${publicKeyHex.take(12)}",
    shortNpub = "npub1${publicKeyHex.take(12)}",
    label = "Account ${publicKeyHex.take(2)}",
    keyAvailability = "available",
    selected = selected,
)
