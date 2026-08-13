package org.harvestcircle.ui.shell

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.HarvestCircleRoute
import org.harvestcircle.application.IdentityEntryMode
import org.harvestcircle.application.ImportSecretDraft
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.RemovalStatus
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.HarvestCircleUiModel
import org.harvestcircle.navigation.BootstrapStep
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class BootstrapIdentityEntryTest {
    @Test
    fun importIsMaskedFocusedBoundedAndCancelable() =
        runComposeUiTest {
            var draft by mutableStateOf("")
            var back = 0
            setContent {
                BootstrapIdentityEntry(
                    BootstrapStep.ImportIdentity,
                    model = model(draft),
                    actions = HarvestCircleUiActions(editImportDraft = { draft = it }),
                    onBack = { back += 1 },
                )
            }
            onNodeWithTag("import-nsec-input").assertIsFocused().performTextInput("nsec1secret")
            assertEquals("nsec1secret", draft)
            onNodeWithTag("identity-entry-cancel").performClick()
            assertEquals(1, back)
        }

    @Test
    fun createDispatchesOnlyTheExistingGenerateAction() =
        runComposeUiTest {
            var generate = 0
            setContent {
                BootstrapIdentityEntry(
                    BootstrapStep.CreateIdentity,
                    model = model(),
                    actions = HarvestCircleUiActions(generateIdentity = { generate += 1 }),
                    onBack = {},
                )
            }
            onNodeWithTag("generate-key").performClick()
            assertEquals(1, generate)
        }

    @Test
    fun hcEx004ImportScreenExplainsTemporaryCustodyTruthfully() =
        runComposeUiTest {
            setContent {
                BootstrapIdentityEntry(
                    BootstrapStep.ImportIdentity,
                    model = model(),
                    actions = HarvestCircleUiActions(),
                    onBack = {},
                )
            }

            onNodeWithText(
                "The secret is held only for this import.\n\n" +
                    "It is cleared after it is sent to the local native runtime.",
            ).assertExists()
            onNodeWithText(
                "The secret is sent directly to the local native runtime " +
                    "and is not retained in the interface.",
            ).assertDoesNotExist()
            onNodeWithTag("import-nsec-input").assertIsFocused()
        }
}

private fun model(importDraft: String = "") =
    HarvestCircleUiModel(
        route = HarvestCircleRoute.IDENTITIES,
        identities = emptyList(),
        activeIdentity = null,
        configuredRelays = emptyList(),
        importDraft = ImportSecretDraft.from(importDraft),
        generatedKeyBackup = null,
        removalConfirmation = null,
        removalStatus = RemovalStatus.NONE,
        lastRemovedPublicKeyHex = null,
        identityChooserVisible = false,
        identityEntryMode = IdentityEntryMode.IMPORT,
        session = SessionLifecycle.SignedOut,
        busy = false,
        problem = null,
        importGuidance = null,
        recoveryAction = RecoveryAction.None,
    )
