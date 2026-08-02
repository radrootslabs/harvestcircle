package org.radroots.studio.accounts.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
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
