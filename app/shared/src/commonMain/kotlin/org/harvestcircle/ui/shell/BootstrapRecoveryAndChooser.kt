package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.HarvestCircleUiModel
import org.harvestcircle.identities.ui.IdentityUiModel

@Composable
fun GeneratedRecoveryCanvas(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
    platformActions: HarvestCirclePlatformActions,
) {
    val backup = requireNotNull(model.generatedKeyBackup)
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { BasicText("Save your recovery key") },
        body = {
            Column(Modifier.testTag("generated-key-backup"), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                BasicText("This key is shown once.")
                BasicText("Store it somewhere private before continuing.")
                BasicText("Recovery key")
                BasicText(backup.nsec, Modifier.testTag("generated-nsec"))
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                ShellAction("Copy recovery key", "Copy recovery key", "copy-generated-key") {
                    platformActions.copySecret(backup.nsec)
                }
                ShellAction("I have saved the recovery key", "I have saved the recovery key", "acknowledge-key-backup") {
                    actions.acknowledgeGeneratedKeyBackup()
                }
                ShellAction("Cancel identity creation", "Cancel identity creation", "cancel-generated-key") {
                    actions.cancelGeneratedKeyBackup()
                }
            }
        },
    )
}

@Composable
fun IdentityChooserCanvas(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
    onReadOnly: () -> Unit,
) {
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { BasicText("Choose a Nostr identity") },
        body = {
            LazyColumn(Modifier.fillMaxWidth().testTag("saved-identity-list")) {
                items(model.identities, key = IdentityUiModel::publicKeyHex) { identity ->
                    IdentityRow(identity, model, actions)
                }
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                ShellAction("Create another identity", "Create another identity", "choose-create-identity") {
                    actions.chooseCreateIdentity()
                }
                ShellAction("Import identity", "Import identity", "choose-import-identity") {
                    actions.chooseImportIdentity()
                }
                ShellAction("Explore read-only", "Explore read-only", "chooser-read-only", onClick = onReadOnly)
            }
        },
    )
}

@Composable
private fun IdentityRow(
    identity: IdentityUiModel,
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .semantics { selected = identity.selected }
            .testTag("identity-row:${identity.publicKeyHex}"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BasicText(identity.label)
        BasicText(identity.shortNpub)
        BasicText(
            if (identity.signerAvailability == "available") {
                "Local credential available"
            } else {
                "Local credential unavailable"
            },
        )
        if (identity.selected) BasicText("Selected")
        ShellAction(
            label = if (identity.selected) "Selected identity" else "Select identity",
            description = "Select ${identity.label}",
            tag = "select-identity:${identity.publicKeyHex}",
            enabled = !model.busy && !identity.selected,
        ) {
            actions.selectIdentity(identity.publicKeyHex)
        }
        ShellAction(
            label = if (identity.active) "Active identity" else "Activate identity",
            description = "Activate ${identity.label}",
            tag = "activate-identity:${identity.publicKeyHex}",
            enabled = !model.busy && !identity.active,
        ) {
            actions.activateIdentity(identity.publicKeyHex)
        }
        ShellAction(
            "Remove local identity",
            "Remove ${identity.label}",
            "remove-identity:${identity.publicKeyHex}",
            enabled = !model.busy,
        ) {
            actions.requestIdentityRemoval(identity.publicKeyHex)
        }
        if (model.pendingRemovalPublicKeyHex == identity.publicKeyHex) {
            BasicText("Remove this saved identity?")
            model.removalImpact?.takeIf { it.deletesLocalCredential }?.let {
                BasicText("Its local credential will be deleted from the operating-system keyring.")
            }
            model.removalImpact?.takeIf { it.signsOut }?.let {
                BasicText("The active session will be signed out before removal.")
            }
            ShellAction("Keep identity", "Keep identity", "remove-cancel") { actions.cancelIdentityRemoval() }
            ShellAction(
                "Remove local identity",
                "Remove local identity",
                "remove-confirm",
                enabled = !model.busy,
            ) {
                actions.confirmIdentityRemoval()
            }
        }
    }
}
