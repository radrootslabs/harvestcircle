package org.harvestcircle.identities.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.HarvestCircleRoute
import org.harvestcircle.application.IdentityEntryMode
import org.harvestcircle.application.IdentityId
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.RemovalImpactState
import org.harvestcircle.application.RemovalStatus
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.UnixSeconds
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class HarvestCircleScreenTest {
    @Test
    fun rendersEveryNonReadyLifecycleRouteWithoutIdentityControls() =
        runComposeUiTest {
            var model by mutableStateOf(emptyUiModel().copy(route = HarvestCircleRoute.OPENING))
            setContent { HarvestCircleScreen(model, HarvestCircleUiActions()) }

            val routes =
                listOf(
                    HarvestCircleRoute.OPENING to "lifecycle-opening",
                    HarvestCircleRoute.CHECKING_COMPATIBILITY to "lifecycle-compatibility",
                    HarvestCircleRoute.ACQUIRING_OWNERSHIP to "lifecycle-ownership",
                    HarvestCircleRoute.MIGRATING to "lifecycle-migrating",
                    HarvestCircleRoute.RECOVERING to "lifecycle-recovering",
                    HarvestCircleRoute.BLOCKED to "lifecycle-blocked",
                    HarvestCircleRoute.SHUTTING_DOWN to "lifecycle-shutting-down",
                    HarvestCircleRoute.FATAL to "lifecycle-fatal",
                    HarvestCircleRoute.CLOSED to "lifecycle-closed",
                )
            routes.forEach { (route, tag) ->
                model = emptyUiModel(problem = "Safe lifecycle problem").copy(route = route)
                waitForIdle()
                onNodeWithTag(tag).assertIsDisplayed()
                onAllNodesWithTag("generate-key").assertCountEquals(0)
            }

            model = emptyUiModel(problem = "Relay access is unavailable.").copy(route = HarvestCircleRoute.DEGRADED)
            waitForIdle()
            onNodeWithTag("identities-screen").assertIsDisplayed()
            onNodeWithTag("identities-problem").assertIsDisplayed()
        }

    @Test
    fun inactiveScreenGeneratesAndImportsMaskedSecretInput() =
        runComposeUiTest {
            var importDraft by mutableStateOf("")
            var identityEntryMode by mutableStateOf(IdentityEntryMode.CHOICE)
            var generateCalls = 0
            var importCalls = 0
            setContent {
                HarvestCircleScreen(
                    model = emptyUiModel(importDraft = importDraft).copy(identityEntryMode = identityEntryMode),
                    actions =
                        HarvestCircleUiActions(
                            chooseCreateIdentity = { identityEntryMode = IdentityEntryMode.CREATE },
                            chooseImportIdentity = { identityEntryMode = IdentityEntryMode.IMPORT },
                            cancelIdentityEntry = { identityEntryMode = IdentityEntryMode.CHOICE },
                            editImportDraft = { importDraft = it },
                            generateIdentity = { generateCalls += 1 },
                            importSecretKey = { importCalls += 1 },
                        ),
                )
            }

            onNodeWithTag("identities-screen").assertIsDisplayed()
            onNodeWithText("HarvestCircle").assertIsDisplayed()
            onNodeWithTag("choose-create-identity").performClick()
            onNodeWithTag("generate-key").performClick()
            onNodeWithTag("cancel-identity-entry").performClick()
            onNodeWithTag("choose-import-identity").performClick()
            onNodeWithTag("import-nsec-input").assertIsFocused()
            onNodeWithTag("import-nsec-input").performTextInput("nsec1secret")
            onNodeWithTag("import-key").performClick()

            assertEquals(1, generateCalls)
            assertEquals(1, importCalls)
            assertEquals("nsec1secret", importDraft)
            assertTrue(
                onNodeWithTag("import-nsec-input").fetchSemanticsNode().config.any {
                    it.key.name == "Password" && it.value == Unit
                },
            )
        }

    @Test
    fun inactiveScreenShowsSafeFailureAndNoGenericFields() =
        runComposeUiTest {
            setContent {
                HarvestCircleScreen(
                    model = emptyUiModel(problem = "The secret key is invalid."),
                    actions = HarvestCircleUiActions(),
                )
            }

            onNodeWithText("The secret key is invalid.").assertIsDisplayed()
            onNodeWithTag("identities-empty").assertIsDisplayed()
        }

    @Test
    fun generatedKeyBackupCopiesAndClearsOnlyAfterAcknowledgement() =
        runComposeUiTest {
            var backup: GeneratedKeyBackupUiModel? by mutableStateOf(
                GeneratedKeyBackupUiModel("npub1generated", "nsec1generated"),
            )
            var copied: String? = null
            setContent {
                HarvestCircleScreen(
                    model = emptyUiModel().copy(generatedKeyBackup = backup),
                    actions =
                        HarvestCircleUiActions(
                            copyText = { copied = it },
                            acknowledgeGeneratedKeyBackup = { backup = null },
                        ),
                )
            }

            onNodeWithTag("generated-key-backup").assertIsDisplayed()
            onAllNodesWithTag("identities-screen").assertCountEquals(0)
            onAllNodesWithTag("generate-key").assertCountEquals(0)
            onNodeWithTag("generated-nsec").assertIsDisplayed()
            onNodeWithTag("copy-generated-key").performClick()
            assertEquals("nsec1generated", copied)

            onNodeWithTag("acknowledge-key-backup").performClick()
            onAllNodesWithTag("generated-key-backup").assertCountEquals(0)
            onAllNodesWithTag("generated-nsec").assertCountEquals(0)
        }

    @Test
    fun generatedKeyRecoveryCanBeCancelledWithoutExposingIdentityControls() =
        runComposeUiTest {
            var backup: GeneratedKeyBackupUiModel? by mutableStateOf(
                GeneratedKeyBackupUiModel("npub1generated", "nsec1generated"),
            )
            var cancelled = 0
            setContent {
                HarvestCircleScreen(
                    model = emptyUiModel().copy(generatedKeyBackup = backup),
                    actions =
                        HarvestCircleUiActions(
                            cancelGeneratedKeyBackup = {
                                cancelled += 1
                                backup = null
                            },
                        ),
                )
            }

            onNodeWithTag("cancel-generated-key").performClick()
            assertEquals(1, cancelled)
            onAllNodesWithTag("generated-key-backup").assertCountEquals(0)
        }

    @Test
    fun savedIdentitiesSelectActivateAndRequireRemovalConfirmation() =
        runComposeUiTest {
            val first = identityUi("11".repeat(32), selected = true)
            val second = identityUi("22".repeat(32), selected = false)
            var pendingRemoval: String? by mutableStateOf(null)
            val selected = mutableListOf<String>()
            val activated = mutableListOf<String>()
            var confirmations = 0
            setContent {
                HarvestCircleScreen(
                    model =
                        emptyUiModel().copy(
                            identities = listOf(first, second),
                            pendingRemovalPublicKeyHex = pendingRemoval,
                            removalImpact =
                                pendingRemoval?.let {
                                    RemovalImpactState(
                                        IdentityId.fromPublicKeyHex(it),
                                        deletesLocalCredential = true,
                                        signsOut = true,
                                        expiresAt = UnixSeconds(60),
                                    )
                                },
                        ),
                    actions =
                        HarvestCircleUiActions(
                            selectIdentity = selected::add,
                            activateIdentity = activated::add,
                            requestIdentityRemoval = { pendingRemoval = it },
                            cancelIdentityRemoval = { pendingRemoval = null },
                            confirmIdentityRemoval = { confirmations += 1 },
                        ),
                )
            }

            onNodeWithTag("saved-identity-list").assertIsDisplayed()
            onNodeWithTag("identity-row:${first.publicKeyHex}").assertIsSelected()
            onNodeWithTag("select-identity:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            onNodeWithTag("activate-identity:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            assertEquals(listOf(second.publicKeyHex), selected)
            assertEquals(listOf(second.publicKeyHex), activated)

            onNodeWithTag("remove-identity:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            onNodeWithText("Its local credential will be deleted from the operating-system keyring.").assertIsDisplayed()
            onNodeWithText("The active session will be signed out before removal.").assertIsDisplayed()
            onNodeWithTag("remove-cancel", useUnmergedTree = true).performClick()
            assertEquals(null, pendingRemoval)
            onNodeWithTag("remove-identity:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            onNodeWithTag("remove-confirm", useUnmergedTree = true).performClick()
            assertEquals(1, confirmations)
        }

    @Test
    fun savedIdentityListRemainsReachableForLargeRegistries() =
        runComposeUiTest {
            val identities =
                (0 until 100).map { index ->
                    identityUi(index.toString(16).padStart(64, '0'), selected = index == 0)
                }
            setContent {
                HarvestCircleScreen(
                    model = emptyUiModel().copy(identities = identities),
                    actions = HarvestCircleUiActions(),
                )
            }

            val lastTag = "identity-row:${identities.last().publicKeyHex}"
            onNodeWithTag("saved-identity-list").performScrollToNode(hasTestTag(lastTag))
            onNodeWithTag(lastTag).assertIsDisplayed()
        }

    @Test
    fun activeHomeShowsIdentityProfileRelayAndCommands() =
        runComposeUiTest {
            var refreshCalls = 0
            var signOutCalls = 0
            val identity = identityUi("33".repeat(32), selected = true)
            val active =
                ActiveIdentityUiModel(
                    identity = identity,
                    heading = "Alice",
                    relayState = "connected",
                    profileState = "fresh",
                    profile =
                        ProfileUiModel(
                            name = "alice",
                            displayName = "Alice",
                            nip05 = "alice@example.com",
                            about = "Local grower",
                            picture = "https://example.com/alice.png",
                        ),
                )
            setContent {
                HarvestCircleScreen(
                    model =
                        emptyUiModel().copy(
                            route = HarvestCircleRoute.ACTIVE_IDENTITY,
                            identities = listOf(identity),
                            activeIdentity = active,
                            configuredRelays = listOf("ws://localhost:8080"),
                            session = SessionLifecycle.Active,
                        ),
                    actions =
                        HarvestCircleUiActions(
                            refreshActiveProfile = { refreshCalls += 1 },
                            signOut = { signOutCalls += 1 },
                        ),
                )
            }

            onNodeWithTag("home-screen").assertIsDisplayed()
            onNodeWithTag("active-npub").assertIsDisplayed()
            onNodeWithTag("active-pubkey-hex").assertIsDisplayed()
            onNodeWithTag("active-profile-name").assertIsDisplayed()
            onNodeWithTag("active-profile-about").assertIsDisplayed()
            onNodeWithTag("relay-state").assertIsDisplayed()
            onNodeWithTag("profile-state").assertIsDisplayed()
            onNodeWithText("ws://localhost:8080").assertIsDisplayed()
            onNodeWithTag("refresh-profile").performClick()
            onNodeWithTag("sign-out").performClick()
            assertEquals(1, refreshCalls)
            assertEquals(1, signOutCalls)
        }

    @Test
    fun activeIdentityCanOpenChooserWithoutDroppingCurrentSession() =
        runComposeUiTest {
            val first = identityUi("44".repeat(32), selected = true, active = true)
            val second = identityUi("55".repeat(32), selected = false)
            val active =
                ActiveIdentityUiModel(
                    identity = first,
                    heading = first.label,
                    relayState = "connected",
                    profileState = "cached",
                    profile = ProfileUiModel("", "", "", "", ""),
                )
            var chooserVisible by mutableStateOf(false)
            var activated: String? = null
            setContent {
                HarvestCircleScreen(
                    model =
                        emptyUiModel().copy(
                            route = HarvestCircleRoute.ACTIVE_IDENTITY,
                            identities = listOf(first, second),
                            activeIdentity = active,
                            session = SessionLifecycle.Active,
                            identityChooserVisible = chooserVisible,
                        ),
                    actions =
                        HarvestCircleUiActions(
                            showIdentityChooser = { chooserVisible = true },
                            hideIdentityChooser = { chooserVisible = false },
                            activateIdentity = { activated = it },
                        ),
                )
            }

            onNodeWithTag("switch-identity").performClick()
            onNodeWithTag("identities-screen").assertIsDisplayed()
            onNodeWithTag("activate-identity:${first.publicKeyHex}", useUnmergedTree = true).assertIsNotEnabled()
            onNodeWithText("Active").assertIsDisplayed()
            onNodeWithTag("activate-identity:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            assertEquals(second.publicKeyHex, activated)
            assertEquals(
                SessionLifecycle.Active,
                emptyUiModel()
                    .copy(
                        activeIdentity = active,
                        session = SessionLifecycle.Active,
                    ).session,
            )
            onNodeWithTag("return-home").performClick()
            onNodeWithTag("home-screen").assertIsDisplayed()
        }
}

private fun emptyUiModel(
    importDraft: String = "",
    problem: String? = null,
    importGuidance: String? = null,
    recoveryAction: RecoveryAction = RecoveryAction.None,
) = HarvestCircleUiModel(
    route = HarvestCircleRoute.IDENTITIES,
    identities = emptyList(),
    activeIdentity = null,
    configuredRelays = emptyList(),
    importDraft = importDraft,
    generatedKeyBackup = null,
    pendingRemovalPublicKeyHex = null,
    removalImpact = null,
    removalStatus = RemovalStatus.NONE,
    lastRemovedPublicKeyHex = null,
    identityChooserVisible = false,
    identityEntryMode = IdentityEntryMode.CHOICE,
    session = SessionLifecycle.SignedOut,
    busy = false,
    problem = problem,
    importGuidance = importGuidance,
    recoveryAction = recoveryAction,
)

private fun identityUi(
    publicKeyHex: String,
    selected: Boolean,
    active: Boolean = false,
) = IdentityUiModel(
    publicKeyHex = publicKeyHex,
    npub = "npub1${publicKeyHex.take(12)}",
    shortNpub = "npub1${publicKeyHex.take(12)}",
    label = "Identity ${publicKeyHex.take(2)}",
    signerAvailability = "available",
    selected = selected,
    active = active,
)
