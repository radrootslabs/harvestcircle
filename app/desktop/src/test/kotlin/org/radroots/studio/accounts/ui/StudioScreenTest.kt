package org.radroots.studio.accounts.ui

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
import org.radroots.studio.application.AccountEntryMode
import org.radroots.studio.application.RemovalImpactState
import org.radroots.studio.application.RemovalStatus
import org.radroots.studio.application.StudioRoute
import org.radroots.studio.ffi.SessionStateDto
import org.radroots.studio.ffi.WireRecoveryAction
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class StudioScreenTest {
    @Test
    fun rendersEveryNonReadyLifecycleRouteWithoutAccountControls() =
        runComposeUiTest {
            var model by mutableStateOf(emptyUiModel().copy(route = StudioRoute.OPENING))
            setContent { StudioScreen(model, StudioUiActions()) }

            val routes =
                listOf(
                    StudioRoute.OPENING to "lifecycle-opening",
                    StudioRoute.CHECKING_COMPATIBILITY to "lifecycle-compatibility",
                    StudioRoute.ACQUIRING_OWNERSHIP to "lifecycle-ownership",
                    StudioRoute.MIGRATING to "lifecycle-migrating",
                    StudioRoute.RECOVERING to "lifecycle-recovering",
                    StudioRoute.BLOCKED to "lifecycle-blocked",
                    StudioRoute.SHUTTING_DOWN to "lifecycle-shutting-down",
                    StudioRoute.FATAL to "lifecycle-fatal",
                    StudioRoute.CLOSED to "lifecycle-closed",
                )
            routes.forEach { (route, tag) ->
                model = emptyUiModel(problem = "Safe lifecycle problem").copy(route = route)
                waitForIdle()
                onNodeWithTag(tag).assertIsDisplayed()
                onAllNodesWithTag("generate-key").assertCountEquals(0)
            }

            model = emptyUiModel(problem = "Relay access is unavailable.").copy(route = StudioRoute.DEGRADED)
            waitForIdle()
            onNodeWithTag("accounts-screen").assertIsDisplayed()
            onNodeWithTag("accounts-problem").assertIsDisplayed()
        }

    @Test
    fun inactiveScreenGeneratesAndImportsMaskedSecretInput() =
        runComposeUiTest {
            var importDraft by mutableStateOf("")
            var accountEntryMode by mutableStateOf(AccountEntryMode.CHOICE)
            var generateCalls = 0
            var importCalls = 0
            setContent {
                StudioScreen(
                    model = emptyUiModel(importDraft = importDraft).copy(accountEntryMode = accountEntryMode),
                    actions =
                        StudioUiActions(
                            chooseCreateAccount = { accountEntryMode = AccountEntryMode.CREATE },
                            chooseImportAccount = { accountEntryMode = AccountEntryMode.IMPORT },
                            cancelAccountEntry = { accountEntryMode = AccountEntryMode.CHOICE },
                            editImportDraft = { importDraft = it },
                            generateAccount = { generateCalls += 1 },
                            importSecretKey = { importCalls += 1 },
                        ),
                )
            }

            onNodeWithTag("accounts-screen").assertIsDisplayed()
            onNodeWithText("radroots").assertIsDisplayed()
            onNodeWithTag("choose-create-account").performClick()
            onNodeWithTag("generate-key").performClick()
            onNodeWithTag("cancel-account-entry").performClick()
            onNodeWithTag("choose-import-account").performClick()
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
                StudioScreen(
                    model = emptyUiModel(problem = "The secret key is invalid."),
                    actions = StudioUiActions(),
                )
            }

            onNodeWithText("The secret key is invalid.").assertIsDisplayed()
            onNodeWithTag("accounts-empty").assertIsDisplayed()
        }

    @Test
    fun generatedKeyBackupCopiesAndClearsOnlyAfterAcknowledgement() =
        runComposeUiTest {
            var backup: GeneratedKeyBackupUiModel? by mutableStateOf(
                GeneratedKeyBackupUiModel("npub1generated", "nsec1generated"),
            )
            var copied: String? = null
            setContent {
                StudioScreen(
                    model = emptyUiModel().copy(generatedKeyBackup = backup),
                    actions =
                        StudioUiActions(
                            copyText = { copied = it },
                            acknowledgeGeneratedKeyBackup = { backup = null },
                        ),
                )
            }

            onNodeWithTag("generated-key-backup").assertIsDisplayed()
            onAllNodesWithTag("accounts-screen").assertCountEquals(0)
            onAllNodesWithTag("generate-key").assertCountEquals(0)
            onNodeWithTag("generated-nsec").assertIsDisplayed()
            onNodeWithTag("copy-generated-key").performClick()
            assertEquals("nsec1generated", copied)

            onNodeWithTag("acknowledge-key-backup").performClick()
            onAllNodesWithTag("generated-key-backup").assertCountEquals(0)
            onAllNodesWithTag("generated-nsec").assertCountEquals(0)
        }

    @Test
    fun generatedKeyRecoveryCanBeCancelledWithoutExposingAccountControls() =
        runComposeUiTest {
            var backup: GeneratedKeyBackupUiModel? by mutableStateOf(
                GeneratedKeyBackupUiModel("npub1generated", "nsec1generated"),
            )
            var cancelled = 0
            setContent {
                StudioScreen(
                    model = emptyUiModel().copy(generatedKeyBackup = backup),
                    actions =
                        StudioUiActions(
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
    fun savedAccountsSelectActivateAndRequireRemovalConfirmation() =
        runComposeUiTest {
            val first = accountUi("11".repeat(32), selected = true)
            val second = accountUi("22".repeat(32), selected = false)
            var pendingRemoval: String? by mutableStateOf(null)
            val selected = mutableListOf<String>()
            val activated = mutableListOf<String>()
            var confirmations = 0
            setContent {
                StudioScreen(
                    model =
                        emptyUiModel().copy(
                            accounts = listOf(first, second),
                            pendingRemovalPublicKeyHex = pendingRemoval,
                            removalImpact =
                                pendingRemoval?.let {
                                    RemovalImpactState(it, deletesLocalCredential = true, signsOut = true, expiresAtSeconds = 60)
                                },
                        ),
                    actions =
                        StudioUiActions(
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
            onNodeWithTag("select-account:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            onNodeWithTag("activate-account:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            assertEquals(listOf(second.publicKeyHex), selected)
            assertEquals(listOf(second.publicKeyHex), activated)

            onNodeWithTag("remove-account:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            onNodeWithText("Its local credential will be deleted from the operating-system keyring.").assertIsDisplayed()
            onNodeWithText("The active session will be signed out before removal.").assertIsDisplayed()
            onNodeWithTag("remove-cancel", useUnmergedTree = true).performClick()
            assertEquals(null, pendingRemoval)
            onNodeWithTag("remove-account:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            onNodeWithTag("remove-confirm", useUnmergedTree = true).performClick()
            assertEquals(1, confirmations)
        }

    @Test
    fun savedAccountListRemainsReachableForLargeRegistries() =
        runComposeUiTest {
            val accounts =
                (0 until 100).map { index ->
                    accountUi(index.toString(16).padStart(64, '0'), selected = index == 0)
                }
            setContent {
                StudioScreen(
                    model = emptyUiModel().copy(accounts = accounts),
                    actions = StudioUiActions(),
                )
            }

            val lastTag = "account-row:${accounts.last().publicKeyHex}"
            onNodeWithTag("saved-account-list").performScrollToNode(hasTestTag(lastTag))
            onNodeWithTag(lastTag).assertIsDisplayed()
        }

    @Test
    fun activeHomeShowsIdentityProfileRelayAndCommands() =
        runComposeUiTest {
            var refreshCalls = 0
            var signOutCalls = 0
            val account = accountUi("33".repeat(32), selected = true)
            val active =
                ActiveAccountUiModel(
                    account = account,
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
                StudioScreen(
                    model =
                        emptyUiModel().copy(
                            route = StudioRoute.ACTIVE_ACCOUNT,
                            accounts = listOf(account),
                            activeAccount = active,
                            configuredRelays = listOf("ws://localhost:8080"),
                            session = SessionStateDto.ACTIVE,
                        ),
                    actions =
                        StudioUiActions(
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
    fun activeAccountCanOpenChooserWithoutDroppingCurrentSession() =
        runComposeUiTest {
            val first = accountUi("44".repeat(32), selected = true, active = true)
            val second = accountUi("55".repeat(32), selected = false)
            val active =
                ActiveAccountUiModel(
                    account = first,
                    heading = first.label,
                    relayState = "connected",
                    profileState = "cached",
                    profile = ProfileUiModel("", "", "", "", ""),
                )
            var chooserVisible by mutableStateOf(false)
            var activated: String? = null
            setContent {
                StudioScreen(
                    model =
                        emptyUiModel().copy(
                            route = StudioRoute.ACTIVE_ACCOUNT,
                            accounts = listOf(first, second),
                            activeAccount = active,
                            session = SessionStateDto.ACTIVE,
                            accountChooserVisible = chooserVisible,
                        ),
                    actions =
                        StudioUiActions(
                            showAccountChooser = { chooserVisible = true },
                            hideAccountChooser = { chooserVisible = false },
                            activateAccount = { activated = it },
                        ),
                )
            }

            onNodeWithTag("switch-account").performClick()
            onNodeWithTag("accounts-screen").assertIsDisplayed()
            onNodeWithTag("activate-account:${first.publicKeyHex}", useUnmergedTree = true).assertIsNotEnabled()
            onNodeWithText("Active").assertIsDisplayed()
            onNodeWithTag("activate-account:${second.publicKeyHex}", useUnmergedTree = true).performClick()
            assertEquals(second.publicKeyHex, activated)
            assertEquals(
                SessionStateDto.ACTIVE,
                emptyUiModel()
                    .copy(
                        activeAccount = active,
                        session = SessionStateDto.ACTIVE,
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
    recoveryAction: WireRecoveryAction = WireRecoveryAction.NONE,
) = StudioUiModel(
    route = StudioRoute.ACCOUNTS,
    accounts = emptyList(),
    activeAccount = null,
    configuredRelays = emptyList(),
    importDraft = importDraft,
    generatedKeyBackup = null,
    pendingRemovalPublicKeyHex = null,
    removalImpact = null,
    removalStatus = RemovalStatus.NONE,
    lastRemovedPublicKeyHex = null,
    accountChooserVisible = false,
    accountEntryMode = AccountEntryMode.CHOICE,
    session = SessionStateDto.SIGNED_OUT,
    busy = false,
    problem = problem,
    importGuidance = importGuidance,
    recoveryAction = recoveryAction,
)

private fun accountUi(
    publicKeyHex: String,
    selected: Boolean,
    active: Boolean = false,
) = AccountUiModel(
    publicKeyHex = publicKeyHex,
    npub = "npub1${publicKeyHex.take(12)}",
    shortNpub = "npub1${publicKeyHex.take(12)}",
    label = "Account ${publicKeyHex.take(2)}",
    keyAvailability = "available",
    selected = selected,
    active = active,
)
