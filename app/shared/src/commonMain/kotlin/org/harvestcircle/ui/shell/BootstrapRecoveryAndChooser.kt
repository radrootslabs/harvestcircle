package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
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
    val backup = model.generatedKeyBackup ?: return
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { ShellText("Save your recovery key", textRole = ShellTextRole.ScreenTitle) },
        body = {
            Column(Modifier.testTag("generated-key-backup"), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                ShellText("This key is shown once.")
                ShellText("Store it somewhere private before continuing.")
                ShellText("Recovery key", textRole = ShellTextRole.CardTitle)
                ShellText(backup.nsec, Modifier.testTag("generated-nsec"), ShellTextRole.Protocol)
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
        header = { ShellText("Choose a Nostr identity", textRole = ShellTextRole.ScreenTitle) },
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
        ShellText(identity.label, textRole = ShellTextRole.CardTitle)
        ShellText(identity.shortNpub, textRole = ShellTextRole.Protocol)
        ShellText(
            if (identity.signerAvailability == org.harvestcircle.application.SignerAvailability.Available) {
                "Local credential available"
            } else {
                "Local credential unavailable"
            },
        )
        if (identity.selected) ShellBadge("Selected")
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
    }
}
