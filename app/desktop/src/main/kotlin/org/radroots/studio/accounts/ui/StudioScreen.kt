package org.radroots.studio.accounts.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.password
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import org.radroots.studio.application.StudioRoute
import org.radroots.studio.application.AccountEntryMode

private val WindowBackgroundColor = Color(0xFFF5F5F2)
private val ButtonBackgroundColor = Color(0xFFE7E7E2)
private val InputBackgroundColor = Color(0xFFFEFDF8)

data class StudioUiActions(
    val chooseCreateAccount: () -> Unit = {},
    val chooseImportAccount: () -> Unit = {},
    val cancelAccountEntry: () -> Unit = {},
    val editImportDraft: (String) -> Unit = {},
    val generateAccount: () -> Unit = {},
    val importSecretKey: () -> Unit = {},
    val copyText: (String) -> Unit = {},
    val acknowledgeGeneratedKeyBackup: () -> Unit = {},
    val cancelGeneratedKeyBackup: () -> Unit = {},
    val selectAccount: (String) -> Unit = {},
    val activateAccount: (String) -> Unit = {},
    val requestAccountRemoval: (String) -> Unit = {},
    val cancelAccountRemoval: () -> Unit = {},
    val confirmAccountRemoval: () -> Unit = {},
    val refreshActiveProfile: () -> Unit = {},
    val signOut: () -> Unit = {},
    val showAccountChooser: () -> Unit = {},
    val hideAccountChooser: () -> Unit = {},
)

@Composable
fun StartupFailureScreen(problem: String) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(WindowBackgroundColor)
            .padding(24.dp)
            .testTag("startup-failure"),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        BasicText("radroots")
        BasicText(problem, Modifier.testTag("startup-problem"))
    }
}

@Composable
fun StudioScreen(
    model: StudioUiModel,
    actions: StudioUiActions,
) {
    model.generatedKeyBackup?.let { backup ->
        GeneratedKeyRecoveryScreen(backup, actions)
        return
    }
    when (model.route) {
        StudioRoute.OPENING -> LifecycleScreen("Opening local account store", "lifecycle-opening")
        StudioRoute.CHECKING_COMPATIBILITY -> LifecycleScreen(
            "Checking native compatibility",
            "lifecycle-compatibility",
        )
        StudioRoute.ACQUIRING_OWNERSHIP -> LifecycleScreen(
            "Acquiring local account store",
            "lifecycle-ownership",
        )
        StudioRoute.MIGRATING -> LifecycleScreen(
            "Updating local account store",
            "lifecycle-migrating",
        )
        StudioRoute.RECOVERING -> LifecycleScreen(
            "Recovering local account state",
            "lifecycle-recovering",
        )
        StudioRoute.SHUTTING_DOWN -> LifecycleScreen("Shutting down", "lifecycle-shutting-down")
        StudioRoute.CLOSED -> LifecycleScreen("Closed", "lifecycle-closed")
        StudioRoute.BLOCKED -> LifecycleScreen(
            model.problem ?: "Local account access is blocked.",
            "lifecycle-blocked",
        )
        StudioRoute.FATAL -> LifecycleScreen(
            model.problem ?: "The application could not continue.",
            "lifecycle-fatal",
        )
        StudioRoute.DEGRADED -> InactiveAccountsScreen(model, actions, degraded = true)
        StudioRoute.ACTIVE_ACCOUNT -> {
            if (model.activeAccount != null && !model.accountChooserVisible) {
                ActiveAccountHome(model, model.activeAccount, actions)
            } else {
                InactiveAccountsScreen(model, actions)
            }
        }
        StudioRoute.ACCOUNTS -> InactiveAccountsScreen(model, actions)
    }
}

@Composable
private fun LifecycleScreen(message: String, testTag: String) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(WindowBackgroundColor)
            .padding(24.dp)
            .testTag(testTag),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        BasicText("radroots")
        BasicText(message)
    }
}

@Composable
private fun ActiveAccountHome(
    model: StudioUiModel,
    active: ActiveAccountUiModel,
    actions: StudioUiActions,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(WindowBackgroundColor)
            .padding(24.dp)
            .testTag("home-screen"),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        BasicText("radroots")
        BasicText(active.heading)
        BasicText(active.account.npub, Modifier.testTag("active-npub"))
        BasicText(active.account.publicKeyHex, Modifier.testTag("active-pubkey-hex"))
        BasicText("Name: ${active.profile.name}", Modifier.testTag("active-profile-name"))
        BasicText("Display name: ${active.profile.displayName}")
        BasicText("NIP-05: ${active.profile.nip05}")
        BasicText("About: ${active.profile.about}", Modifier.testTag("active-profile-about"))
        BasicText("Picture: ${active.profile.picture}")
        BasicText("Relay: ${active.relayState}", Modifier.testTag("relay-state"))
        BasicText("Profile: ${active.profileState}", Modifier.testTag("profile-state"))
        BasicText("Configured relays")
        if (model.configuredRelays.isEmpty()) {
            BasicText("None")
        } else {
            model.configuredRelays.forEach { relay -> BasicText(relay) }
        }
        TextAction(
            text = "Switch account",
            testTag = "switch-account",
            contentDescription = "Choose another saved account",
            enabled = !model.busy,
            onClick = actions.showAccountChooser,
        )
        TextAction(
            text = "Refresh metadata",
            testTag = "refresh-profile",
            contentDescription = "Refresh active Nostr profile metadata",
            enabled = !model.busy,
            onClick = actions.refreshActiveProfile,
        )
        TextAction(
            text = "Sign out",
            testTag = "sign-out",
            contentDescription = "Sign out of the active account",
            enabled = !model.busy,
            onClick = actions.signOut,
        )
        model.problem?.let { BasicText(it, Modifier.testTag("home-problem")) }
    }
}

@Composable
private fun InactiveAccountsScreen(
    model: StudioUiModel,
    actions: StudioUiActions,
    degraded: Boolean = false,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(WindowBackgroundColor)
            .padding(24.dp)
            .testTag("accounts-screen"),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        BasicText("radroots")
        BasicText("Accounts")
        if (degraded) {
            BasicText(model.problem ?: "Nostr relay access is unavailable. Local accounts remain available.")
        }

        if (model.activeAccount != null) {
            BasicText("Choose an account to activate. The current account remains active until replacement succeeds.")
            TextAction(
                text = "Back to active account",
                testTag = "return-home",
                contentDescription = "Return to the active account",
                onClick = actions.hideAccountChooser,
            )
        }

        AccountEntry(model, actions)

        model.problem?.let {
            BasicText(it, Modifier.testTag("accounts-problem"))
        }

        if (model.accounts.isEmpty()) {
            BasicText("No saved accounts.", Modifier.testTag("accounts-empty"))
        } else {
            SavedAccountList(model, actions)
        }
    }
}

@Composable
private fun AccountEntry(model: StudioUiModel, actions: StudioUiActions) {
    when (model.accountEntryMode) {
        AccountEntryMode.CHOICE -> {
            TextAction(
                text = "Create account",
                testTag = "choose-create-account",
                contentDescription = "Create a new Nostr account",
                enabled = !model.busy,
                onClick = actions.chooseCreateAccount,
            )
            TextAction(
                text = "Import key",
                testTag = "choose-import-account",
                contentDescription = "Import an existing Nostr secret key",
                enabled = !model.busy,
                onClick = actions.chooseImportAccount,
            )
        }
        AccountEntryMode.CREATE -> {
            TextAction(
                text = "Back",
                testTag = "cancel-account-entry",
                contentDescription = "Return to account choices",
                enabled = !model.busy,
                onClick = actions.cancelAccountEntry,
            )
            TextAction(
                text = "Generate new key",
                testTag = "generate-key",
                contentDescription = "Generate a new Nostr key",
                enabled = !model.busy && model.generatedKeyBackup == null,
                onClick = actions.generateAccount,
            )
        }
        AccountEntryMode.IMPORT -> {
            TextAction(
                text = "Back",
                testTag = "cancel-account-entry",
                contentDescription = "Return to account choices",
                enabled = !model.busy,
                onClick = actions.cancelAccountEntry,
            )
            BasicTextField(
                value = model.importDraft,
                onValueChange = actions.editImportDraft,
                enabled = !model.busy,
                visualTransformation = PasswordVisualTransformation(),
                modifier = Modifier
                    .fillMaxWidth()
                    .semantics {
                        contentDescription = "Nostr secret key"
                        password()
                    }
                    .testTag("import-nsec-input")
                    .background(InputBackgroundColor)
                    .padding(8.dp),
                decorationBox = { innerTextField ->
                    if (model.importDraft.isEmpty()) BasicText("nsec or secret-key hex")
                    innerTextField()
                },
            )
            TextAction(
                text = "Add existing key",
                testTag = "import-key",
                contentDescription = "Import an existing Nostr secret key",
                enabled = !model.busy && model.importDraft.isNotBlank(),
                onClick = actions.importSecretKey,
            )
        }
    }
}

@Composable
private fun ColumnScope.SavedAccountList(
    model: StudioUiModel,
    actions: StudioUiActions,
) {
    LazyColumn(
        modifier = Modifier
            .fillMaxWidth()
            .weight(1f)
            .testTag("saved-account-list"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(model.accounts, key = AccountUiModel::publicKeyHex) { account ->
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .semantics { selected = account.selected }
                    .testTag("account-row:${account.publicKeyHex}")
                    .clickable { actions.selectAccount(account.publicKeyHex) }
                    .background(InputBackgroundColor)
                    .padding(12.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                BasicText(account.label)
                BasicText(account.npub)
                BasicText("Key: ${account.keyAvailability}")
                if (account.selected) BasicText("Selected")
                TextAction(
                    text = "Activate",
                    testTag = "activate-account:${account.publicKeyHex}",
                    contentDescription = "Activate ${account.label}",
                    enabled = !model.busy,
                    onClick = { actions.activateAccount(account.publicKeyHex) },
                )
                TextAction(
                    text = "Remove",
                    testTag = "remove-account:${account.publicKeyHex}",
                    contentDescription = "Remove ${account.label}",
                    enabled = !model.busy,
                    onClick = { actions.requestAccountRemoval(account.publicKeyHex) },
                )
                if (model.pendingRemovalPublicKeyHex == account.publicKeyHex) {
                    BasicText("Remove this saved account and its local credential?")
                    TextAction(
                        text = "Cancel",
                        testTag = "remove-cancel",
                        contentDescription = "Cancel account removal",
                        onClick = actions.cancelAccountRemoval,
                    )
                    TextAction(
                        text = "Confirm removal",
                        testTag = "remove-confirm",
                        contentDescription = "Confirm account removal",
                        onClick = actions.confirmAccountRemoval,
                    )
                }
            }
        }
    }
}

@Composable
private fun GeneratedKeyRecoveryScreen(
    backup: GeneratedKeyBackupUiModel,
    actions: StudioUiActions,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(WindowBackgroundColor)
            .padding(24.dp)
            .testTag("generated-key-backup"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BasicText("Save this key")
        BasicText("Losing this secret key means losing access to the account.")
        BasicText(backup.npub)
        BasicText(backup.nsec, Modifier.testTag("generated-nsec"))
        TextAction(
            text = "Copy",
            testTag = "copy-generated-key",
            contentDescription = "Copy generated Nostr secret key",
            onClick = { actions.copyText(backup.nsec) },
        )
        TextAction(
            text = "Cancel",
            testTag = "cancel-generated-key",
            contentDescription = "Cancel generated account",
            onClick = actions.cancelGeneratedKeyBackup,
        )
        TextAction(
            text = "I have saved this key",
            testTag = "acknowledge-key-backup",
            contentDescription = "Confirm generated key backup",
            onClick = actions.acknowledgeGeneratedKeyBackup,
        )
    }
}

@Composable
internal fun TextAction(
    text: String,
    testTag: String,
    contentDescription: String,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    BasicText(
        text = text,
        modifier = Modifier
            .semantics {
                role = Role.Button
                this.contentDescription = contentDescription
                if (!enabled) disabled()
            }
            .testTag(testTag)
            .then(if (enabled) Modifier.clickable(onClick = onClick) else Modifier)
            .background(ButtonBackgroundColor)
            .padding(8.dp),
    )
}
