package org.harvestcircle.identities.ui

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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
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
import org.harvestcircle.application.HarvestCircleRoute
import org.harvestcircle.application.IdentityEntryMode

private val WindowBackgroundColor = Color(0xFFF5F5F2)
private val ButtonBackgroundColor = Color(0xFFE7E7E2)
private val InputBackgroundColor = Color(0xFFFEFDF8)

data class HarvestCircleUiActions(
    val chooseCreateIdentity: () -> Unit = {},
    val chooseImportIdentity: () -> Unit = {},
    val cancelIdentityEntry: () -> Unit = {},
    val editImportDraft: (String) -> Unit = {},
    val generateIdentity: () -> Unit = {},
    val importSecretKey: () -> Unit = {},
    val copyText: (String) -> Unit = {},
    val acknowledgeGeneratedKeyBackup: () -> Unit = {},
    val cancelGeneratedKeyBackup: () -> Unit = {},
    val selectIdentity: (String) -> Unit = {},
    val activateIdentity: (String) -> Unit = {},
    val requestIdentityRemoval: (String) -> Unit = {},
    val cancelIdentityRemoval: () -> Unit = {},
    val confirmIdentityRemoval: () -> Unit = {},
    val refreshActiveProfile: () -> Unit = {},
    val retryLastCommand: () -> Unit = {},
    val signOut: () -> Unit = {},
    val showIdentityChooser: () -> Unit = {},
    val hideIdentityChooser: () -> Unit = {},
)

@Composable
fun StartupFailureScreen(problem: String) {
    Column(
        modifier =
            Modifier
                .fillMaxSize()
                .background(WindowBackgroundColor)
                .padding(24.dp)
                .verticalScroll(rememberScrollState())
                .testTag("startup-failure"),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        BasicText("HarvestCircle")
        BasicText(problem, Modifier.testTag("startup-problem"))
    }
}

@Composable
fun HarvestCircleScreen(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
) {
    model.generatedKeyBackup?.let { backup ->
        GeneratedKeyRecoveryScreen(backup, actions)
        return
    }
    when (model.route) {
        HarvestCircleRoute.OPENING -> LifecycleScreen("Opening local identity store", "lifecycle-opening")
        HarvestCircleRoute.CHECKING_COMPATIBILITY ->
            LifecycleScreen(
                "Checking native compatibility",
                "lifecycle-compatibility",
            )
        HarvestCircleRoute.ACQUIRING_OWNERSHIP ->
            LifecycleScreen(
                "Acquiring local identity store",
                "lifecycle-ownership",
            )
        HarvestCircleRoute.MIGRATING ->
            LifecycleScreen(
                "Updating local identity store",
                "lifecycle-migrating",
            )
        HarvestCircleRoute.RECOVERING ->
            LifecycleScreen(
                "Recovering local identity state",
                "lifecycle-recovering",
            )
        HarvestCircleRoute.SHUTTING_DOWN -> LifecycleScreen("Shutting down", "lifecycle-shutting-down")
        HarvestCircleRoute.CLOSED -> LifecycleScreen("Closed", "lifecycle-closed")
        HarvestCircleRoute.BLOCKED ->
            LifecycleScreen(
                model.problem ?: "Local identity access is blocked.",
                "lifecycle-blocked",
            )
        HarvestCircleRoute.FATAL ->
            LifecycleScreen(
                model.problem ?: "The application could not continue.",
                "lifecycle-fatal",
            )
        HarvestCircleRoute.DEGRADED -> InactiveIdentitiesScreen(model, actions, degraded = true)
        HarvestCircleRoute.ACTIVE_IDENTITY -> {
            if (model.activeIdentity != null && !model.identityChooserVisible) {
                ActiveIdentityHome(model, model.activeIdentity, actions)
            } else {
                InactiveIdentitiesScreen(model, actions)
            }
        }
        HarvestCircleRoute.IDENTITYS -> InactiveIdentitiesScreen(model, actions)
    }
}

@Composable
private fun LifecycleScreen(
    message: String,
    testTag: String,
) {
    Column(
        modifier =
            Modifier
                .fillMaxSize()
                .background(WindowBackgroundColor)
                .padding(24.dp)
                .testTag(testTag),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        BasicText("HarvestCircle")
        BasicText(message)
    }
}

@Composable
private fun ActiveIdentityHome(
    model: HarvestCircleUiModel,
    active: ActiveIdentityUiModel,
    actions: HarvestCircleUiActions,
) {
    Column(
        modifier =
            Modifier
                .fillMaxSize()
                .background(WindowBackgroundColor)
                .padding(24.dp)
                .verticalScroll(rememberScrollState())
                .testTag("home-screen"),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        BasicText("HarvestCircle")
        BasicText(active.heading)
        BasicText(active.identity.npub, Modifier.testTag("active-npub"))
        BasicText(active.identity.publicKeyHex, Modifier.testTag("active-pubkey-hex"))
        BasicText("Name: ${active.profile.name}", Modifier.testTag("active-profile-name"))
        BasicText("Display name: ${active.profile.displayName}")
        BasicText("NIP-05 (unverified): ${active.profile.nip05}")
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
            text = "Switch identity",
            testTag = "switch-identity",
            contentDescription = "Choose another saved identity",
            enabled = !model.busy,
            onClick = actions.showIdentityChooser,
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
            contentDescription = "Sign out of the active identity",
            enabled = !model.busy,
            onClick = actions.signOut,
        )
        model.problem?.let { BasicText(it, Modifier.testTag("home-problem")) }
        RecoveryAction(model, actions)
    }
}

@Composable
private fun InactiveIdentitiesScreen(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
    degraded: Boolean = false,
) {
    Column(
        modifier =
            Modifier
                .fillMaxSize()
                .background(WindowBackgroundColor)
                .padding(24.dp)
                .testTag("identities-screen"),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        BasicText("HarvestCircle")
        BasicText("Identities")
        if (degraded) {
            BasicText(model.problem ?: "Nostr relay access is unavailable. Local identities remain available.")
        }

        if (model.activeIdentity != null) {
            BasicText("Choose an identity to activate. The current identity remains active until replacement succeeds.")
            TextAction(
                text = "Back to active identity",
                testTag = "return-home",
                contentDescription = "Return to the active identity",
                onClick = actions.hideIdentityChooser,
            )
        }

        IdentityEntry(model, actions)

        model.problem?.let {
            BasicText(it, Modifier.testTag("identities-problem"))
        }
        RecoveryAction(model, actions)

        if (model.identities.isEmpty()) {
            BasicText("No saved identities.", Modifier.testTag("identities-empty"))
        } else {
            SavedIdentityList(model, actions)
        }
    }
}

@Composable
private fun RecoveryAction(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
) {
    if (model.recoveryAction == org.harvestcircle.ffi.WireRecoveryAction.RETRY) {
        TextAction(
            text = "Retry",
            testTag = "retry-last-command",
            contentDescription = "Retry the last failed action",
            enabled = !model.busy,
            onClick = actions.retryLastCommand,
        )
    }
}

@Composable
private fun IdentityEntry(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
) {
    when (model.identityEntryMode) {
        IdentityEntryMode.CHOICE -> {
            TextAction(
                text = "Create identity",
                testTag = "choose-create-identity",
                contentDescription = "Create a new Nostr identity",
                enabled = !model.busy,
                onClick = actions.chooseCreateIdentity,
            )
            TextAction(
                text = "Import key",
                testTag = "choose-import-identity",
                contentDescription = "Import an existing Nostr secret key",
                enabled = !model.busy,
                onClick = actions.chooseImportIdentity,
            )
        }
        IdentityEntryMode.CREATE -> {
            TextAction(
                text = "Back",
                testTag = "cancel-identity-entry",
                contentDescription = "Return to identity choices",
                enabled = !model.busy,
                onClick = actions.cancelIdentityEntry,
            )
            TextAction(
                text = "Generate new key",
                testTag = "generate-key",
                contentDescription = "Generate a new Nostr key",
                enabled = !model.busy && model.generatedKeyBackup == null,
                onClick = actions.generateIdentity,
            )
        }
        IdentityEntryMode.IMPORT -> {
            val importFocusRequester = remember { FocusRequester() }
            LaunchedEffect(Unit) { importFocusRequester.requestFocus() }
            TextAction(
                text = "Back",
                testTag = "cancel-identity-entry",
                contentDescription = "Return to identity choices",
                enabled = !model.busy,
                onClick = actions.cancelIdentityEntry,
            )
            BasicTextField(
                value = model.importDraft,
                onValueChange = actions.editImportDraft,
                enabled = !model.busy,
                visualTransformation = PasswordVisualTransformation(),
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .semantics {
                            contentDescription = "Nostr secret key"
                            password()
                        }.focusRequester(importFocusRequester)
                        .testTag("import-nsec-input")
                        .background(InputBackgroundColor)
                        .padding(8.dp),
                decorationBox = { innerTextField ->
                    if (model.importDraft.isEmpty()) BasicText("nsec or secret-key hex")
                    innerTextField()
                },
            )
            model.importGuidance?.let { guidance ->
                BasicText(guidance, Modifier.testTag("import-guidance"))
            }
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
private fun ColumnScope.SavedIdentityList(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
) {
    LazyColumn(
        modifier =
            Modifier
                .fillMaxWidth()
                .weight(1f)
                .testTag("saved-identity-list"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(model.identities, key = IdentityUiModel::publicKeyHex) { identity ->
            Column(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .semantics { selected = identity.selected }
                        .testTag("identity-row:${identity.publicKeyHex}")
                        .background(InputBackgroundColor)
                        .padding(12.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                BasicText(identity.label)
                BasicText(identity.npub)
                BasicText("Key: ${identity.signerAvailability}")
                if (identity.selected) BasicText("Selected")
                if (identity.active) BasicText("Active")
                TextAction(
                    text = if (identity.selected) "Selected identity" else "Select",
                    testTag = "select-identity:${identity.publicKeyHex}",
                    contentDescription = "Select ${identity.label}",
                    enabled = !model.busy && !identity.selected,
                    onClick = { actions.selectIdentity(identity.publicKeyHex) },
                )
                TextAction(
                    text = if (identity.active) "Active identity" else "Activate",
                    testTag = "activate-identity:${identity.publicKeyHex}",
                    contentDescription = "Activate ${identity.label}",
                    enabled = !model.busy && !identity.active,
                    onClick = { actions.activateIdentity(identity.publicKeyHex) },
                )
                TextAction(
                    text = "Remove",
                    testTag = "remove-identity:${identity.publicKeyHex}",
                    contentDescription = "Remove ${identity.label}",
                    enabled = !model.busy,
                    onClick = { actions.requestIdentityRemoval(identity.publicKeyHex) },
                )
                if (model.pendingRemovalPublicKeyHex == identity.publicKeyHex) {
                    BasicText("Remove this saved identity?")
                    if (model.removalImpact?.deletesLocalCredential == true) {
                        BasicText("Its local credential will be deleted from the operating-system keyring.")
                    }
                    if (model.removalImpact?.signsOut == true) {
                        BasicText("The active session will be signed out before removal.")
                    }
                    TextAction(
                        text = "Cancel",
                        testTag = "remove-cancel",
                        contentDescription = "Cancel identity removal",
                        onClick = actions.cancelIdentityRemoval,
                    )
                    TextAction(
                        text = "Confirm removal",
                        testTag = "remove-confirm",
                        contentDescription = "Confirm identity removal",
                        enabled = !model.busy,
                        onClick = actions.confirmIdentityRemoval,
                    )
                }
            }
        }
    }
}

@Composable
private fun GeneratedKeyRecoveryScreen(
    backup: GeneratedKeyBackupUiModel,
    actions: HarvestCircleUiActions,
) {
    Column(
        modifier =
            Modifier
                .fillMaxSize()
                .background(WindowBackgroundColor)
                .padding(24.dp)
                .verticalScroll(rememberScrollState())
                .testTag("generated-key-backup"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BasicText("Save this key")
        BasicText("Losing this secret key means losing access to the identity.")
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
            contentDescription = "Cancel generated identity",
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
        modifier =
            Modifier
                .semantics {
                    role = Role.Button
                    this.contentDescription = contentDescription
                    if (!enabled) disabled()
                }.testTag(testTag)
                .then(if (enabled) Modifier.clickable(onClick = onClick) else Modifier)
                .background(ButtonBackgroundColor)
                .padding(8.dp),
    )
}
