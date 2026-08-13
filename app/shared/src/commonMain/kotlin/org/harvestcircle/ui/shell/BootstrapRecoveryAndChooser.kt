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
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.action.HarvestCircleLabeledButton
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBadge
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
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
        header = { HarvestCircleText("Save your recovery key", role = HarvestCircleTextRole.PageTitle) },
        body = {
            Column(
                Modifier.testTag("generated-key-backup"),
                verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
            ) {
                HarvestCircleText("This key is shown once.")
                HarvestCircleText("Store it somewhere private before continuing.")
                HarvestCircleText("Recovery key", role = HarvestCircleTextRole.SubsectionTitle)
                HarvestCircleText(backup.nsec, Modifier.testTag("generated-nsec"), HarvestCircleTextRole.Code)
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                HarvestCircleLabeledButton(
                    "Copy recovery key",
                    "Copy recovery key",
                    { platformActions.copySecret(backup.nsec) },
                    Modifier.testTag("copy-generated-key"),
                )
                HarvestCircleLabeledButton(
                    "I have saved the recovery key",
                    "I have saved the recovery key",
                    { actions.acknowledgeGeneratedKeyBackup() },
                    Modifier.testTag("acknowledge-key-backup"),
                )
                HarvestCircleLabeledButton(
                    "Cancel identity creation",
                    "Cancel identity creation",
                    { actions.cancelGeneratedKeyBackup() },
                    Modifier.testTag("cancel-generated-key"),
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
        header = { HarvestCircleText("Choose a Nostr identity", role = HarvestCircleTextRole.PageTitle) },
        body = {
            LazyColumn(Modifier.fillMaxWidth().testTag("saved-identity-list")) {
                items(model.identities, key = IdentityUiModel::publicKeyHex) { identity ->
                    IdentityRow(identity, model, actions)
                }
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                HarvestCircleLabeledButton(
                    "Create another identity",
                    "Create another identity",
                    { actions.chooseCreateIdentity() },
                    Modifier.testTag("choose-create-identity"),
                )
                HarvestCircleLabeledButton(
                    "Import identity",
                    "Import identity",
                    { actions.chooseImportIdentity() },
                    Modifier.testTag("choose-import-identity"),
                )
                HarvestCircleLabeledButton("Explore read-only", "Explore read-only", onReadOnly, Modifier.testTag("chooser-read-only"))
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
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap),
    ) {
        HarvestCircleText(identity.label, role = HarvestCircleTextRole.SubsectionTitle)
        HarvestCircleText(identity.shortNpub, role = HarvestCircleTextRole.Code)
        HarvestCircleText(
            if (identity.signerAvailability == org.harvestcircle.application.SignerAvailability.Available) {
                "Local credential available"
            } else {
                "Local credential unavailable"
            },
        )
        if (identity.selected) HarvestCircleBadge("Selected")
        HarvestCircleLabeledButton(
            label = if (identity.selected) "Selected identity" else "Select identity",
            accessibilityLabel = "Select ${identity.label}",
            modifier = Modifier.testTag("select-identity:${identity.publicKeyHex}"),
            enabled = !model.busy && !identity.selected,
            onClick = { actions.selectIdentity(identity.publicKeyHex) },
        )
        HarvestCircleLabeledButton(
            label = if (identity.active) "Active identity" else "Activate identity",
            accessibilityLabel = "Activate ${identity.label}",
            modifier = Modifier.testTag("activate-identity:${identity.publicKeyHex}"),
            enabled = !model.busy && !identity.active,
            onClick = { actions.activateIdentity(identity.publicKeyHex) },
        )
        HarvestCircleLabeledButton(
            label = "Remove local identity",
            accessibilityLabel = "Remove ${identity.label}",
            enabled = !model.busy,
            modifier =
                Modifier
                    .shellFocusTarget(ShellFocusTarget.IdentityRow(identity.publicKeyHex))
                    .testTag("remove-identity:${identity.publicKeyHex}"),
            onClick = { actions.requestIdentityRemoval(identity.publicKeyHex) },
        )
    }
}
