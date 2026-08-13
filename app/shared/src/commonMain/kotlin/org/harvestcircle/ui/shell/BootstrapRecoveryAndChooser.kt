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
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.designsystem.component.feedback.HarvestCircleProgressIndicator
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPalette
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
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
        header = { HarvestCircleShellText("Save your recovery key", role = HarvestCircleShellTextRole.PaneTitle) },
        body = {
            HarvestCircleShellPanel(Modifier.testTag("generated-key-backup")) {
                HarvestCircleShellText("This key is shown once.", role = HarvestCircleShellTextRole.SectionTitle)
                HarvestCircleShellText("Store it somewhere private before continuing.")
                HarvestCircleShellText("Recovery key", role = HarvestCircleShellTextRole.Label)
                HarvestCircleShellText(backup.nsec, Modifier.testTag("generated-nsec"), HarvestCircleShellTextRole.Code)
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                HarvestCircleShellButton(
                    "Copy recovery key",
                    { platformActions.copySecret(backup.nsec) },
                    Modifier.testTag("copy-generated-key"),
                )
                HarvestCircleShellButton(
                    "I have saved the recovery key",
                    actions.acknowledgeGeneratedKeyBackup,
                    Modifier.testTag("acknowledge-key-backup"),
                    primary = true,
                )
                HarvestCircleShellButton(
                    "Cancel identity creation",
                    actions.cancelGeneratedKeyBackup,
                    Modifier.testTag("cancel-generated-key"),
                    destructive = true,
                )
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
        header = { HarvestCircleShellText("Choose a Nostr identity", role = HarvestCircleShellTextRole.PaneTitle) },
        body = {
            LazyColumn(
                Modifier.fillMaxWidth().testTag("saved-identity-list"),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                items(model.identities, key = IdentityUiModel::publicKeyHex) { identity -> IdentityRow(identity, model, actions) }
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                HarvestCircleShellButton(
                    "Create another identity",
                    actions.chooseCreateIdentity,
                    Modifier.testTag("choose-create-identity"),
                )
                HarvestCircleShellButton("Import identity", actions.chooseImportIdentity, Modifier.testTag("choose-import-identity"))
                HarvestCircleShellButton("Explore read-only", onReadOnly, Modifier.testTag("chooser-read-only"), primary = true)
            }
        },
    )
}

@Composable
fun IdentityActivationCanvas(
    model: HarvestCircleUiModel,
    activatingPublicKeyHex: String,
) {
    val identity = model.identities.singleOrNull { it.publicKeyHex == activatingPublicKeyHex }
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { HarvestCircleShellText("Activating identity", role = HarvestCircleShellTextRole.PaneTitle) },
        body = {
            Column(
                Modifier
                    .fillMaxWidth()
                    .semantics { liveRegion = LiveRegionMode.Polite }
                    .testTag("identity-activation-progress"),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                HarvestCircleShellPanel {
                    HarvestCircleProgressIndicator(Modifier.testTag("identity-activation-indicator"))
                    HarvestCircleShellText(
                        identity?.label ?: "Selected identity",
                        role = HarvestCircleShellTextRole.SectionTitle,
                    )
                    identity?.let {
                        HarvestCircleShellText(it.shortNpub, role = HarvestCircleShellTextRole.Code)
                    }
                    HarvestCircleShellText(
                        "Checking the local credential and preparing signed actions.",
                        color = HarvestCircleShellPalette.contentSecondary,
                    )
                }
            }
        },
        actionBar = {},
    )
}

@Composable
private fun IdentityRow(
    identity: IdentityUiModel,
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
) {
    HarvestCircleShellPanel(
        Modifier
            .semantics { selected = identity.selected }
            .testTag("identity-row:${identity.publicKeyHex}"),
    ) {
        HarvestCircleShellText(identity.label, role = HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText(identity.shortNpub, role = HarvestCircleShellTextRole.Code)
        HarvestCircleShellText(
            if (identity.signerAvailability == org.harvestcircle.application.SignerAvailability.Available) {
                "Local credential available"
            } else {
                "Local credential unavailable"
            },
            color = HarvestCircleShellPalette.contentSecondary,
        )
        if (identity.selected) HarvestCircleShellText("Selected", role = HarvestCircleShellTextRole.Small)
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            HarvestCircleShellButton(
                if (identity.selected) "Selected identity" else "Select identity",
                { actions.selectIdentity(identity.publicKeyHex) },
                Modifier.testTag("select-identity:${identity.publicKeyHex}"),
                primary = !identity.selected,
                enabled = !model.busy && !identity.selected,
            )
            HarvestCircleShellButton(
                if (identity.active) "Active identity" else "Activate identity",
                { actions.activateIdentity(identity.publicKeyHex) },
                Modifier.testTag("activate-identity:${identity.publicKeyHex}"),
                enabled = !model.busy && !identity.active,
            )
            HarvestCircleShellButton(
                "Remove local identity",
                { actions.requestIdentityRemoval(identity.publicKeyHex) },
                Modifier
                    .shellFocusTarget(ShellFocusTarget.IdentityRow(identity.publicKeyHex))
                    .testTag("remove-identity:${identity.publicKeyHex}"),
                destructive = true,
                enabled = !model.busy,
            )
        }
    }
}
