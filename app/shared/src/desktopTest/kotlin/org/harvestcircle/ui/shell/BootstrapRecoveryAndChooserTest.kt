package org.harvestcircle.ui.shell

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.HarvestCircleRoute
import org.harvestcircle.application.IdentityEntryMode
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.RemovalStatus
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.identities.ui.GeneratedKeyBackupUiModel
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.HarvestCircleUiModel
import org.harvestcircle.identities.ui.IdentityUiModel
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class BootstrapRecoveryAndChooserTest {
    @Test
    fun oneUseRecoveryOwnsCopyAcknowledgementAndCancellation() =
        runComposeUiTest {
            var backup: GeneratedKeyBackupUiModel? by
                mutableStateOf(GeneratedKeyBackupUiModel("npub1generated", "nsec1generated"))
            var copied = ""
            var cancelled = 0
            setHarvestCircleContent {
                backup?.let {
                    GeneratedRecoveryCanvas(
                        model = model(generatedKeyBackup = it),
                        actions =
                            HarvestCircleUiActions(
                                acknowledgeGeneratedKeyBackup = { backup = null },
                                cancelGeneratedKeyBackup = { cancelled += 1 },
                            ),
                        platformActions = HarvestCirclePlatformActions(copySecret = { copied = it }),
                    )
                }
            }

            onAllNodesWithTag("generated-nsec").assertCountEquals(1)
            onNodeWithTag("copy-generated-key").performClick()
            assertEquals("nsec1generated", copied)
            onNodeWithTag("cancel-generated-key").performClick()
            assertEquals(1, cancelled)
            onNodeWithTag("acknowledge-key-backup").performClick()
            onAllNodesWithTag("generated-key-backup").assertCountEquals(0)
            onAllNodesWithTag("generated-nsec").assertCountEquals(0)
        }

    @Test
    fun chooserExposesExplicitSelectionActivationAndRemoval() =
        runComposeUiTest {
            val first = identity("first", selected = true)
            val second = identity("second")
            var selected = ""
            var activated = ""
            var removal = ""
            setHarvestCircleContent {
                IdentityChooserCanvas(
                    model = model(identities = listOf(first, second)),
                    actions =
                        HarvestCircleUiActions(
                            selectIdentity = { selected = it },
                            activateIdentity = { activated = it },
                            requestIdentityRemoval = { removal = it },
                        ),
                    onReadOnly = {},
                )
            }

            onNodeWithTag("select-identity:second").performClick()
            onNodeWithTag("activate-identity:second").performClick()
            onNodeWithTag("remove-identity:second").performClick()
            assertEquals("second", selected)
            assertEquals("second", activated)
            assertEquals("second", removal)
        }

    @Test
    fun activationProgressNamesTheTargetAndExposesIndeterminateStatus() =
        runComposeUiTest {
            setHarvestCircleContent {
                IdentityActivationCanvas(
                    model = model(identities = listOf(identity("first", selected = true))),
                    activatingPublicKeyHex = "first",
                )
            }

            onNodeWithTag("identity-activation-progress").assertIsDisplayed()
            onNodeWithTag("identity-activation-indicator").assertIsDisplayed()
            onNodeWithText("First").assertIsDisplayed()
            onNodeWithText("Checking the local credential and preparing signed actions.").assertIsDisplayed()
        }
}

private fun identity(
    id: String,
    selected: Boolean = false,
) = IdentityUiModel(
    publicKeyHex = id,
    npub = "npub1$id",
    shortNpub = "npub1$id",
    label = id.replaceFirstChar(Char::uppercaseChar),
    signerAvailability = org.harvestcircle.application.SignerAvailability.Available,
    selected = selected,
    active = false,
)

private fun model(
    identities: List<IdentityUiModel> = emptyList(),
    generatedKeyBackup: GeneratedKeyBackupUiModel? = null,
) = HarvestCircleUiModel(
    route = HarvestCircleRoute.IDENTITIES,
    identities = identities,
    activeIdentity = null,
    configuredRelays = emptyList(),
    importDraft =
        org.harvestcircle.application.ImportSecretDraft
            .empty(),
    generatedKeyBackup = generatedKeyBackup,
    removalConfirmation = null,
    removalStatus = RemovalStatus.NONE,
    lastRemovedPublicKeyHex = null,
    identityChooserVisible = false,
    identityEntryMode = IdentityEntryMode.CHOICE,
    session = SessionLifecycle.SignedOut,
    busy = false,
    problem = null,
    importGuidance = null,
    recoveryAction = RecoveryAction.None,
)
