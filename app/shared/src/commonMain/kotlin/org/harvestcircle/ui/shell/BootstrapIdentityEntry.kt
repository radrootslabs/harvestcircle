package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.password
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.HarvestCircleUiModel
import org.harvestcircle.navigation.BootstrapStep

@Composable
fun BootstrapIdentityEntry(
    step: BootstrapStep,
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
    onBack: () -> Unit,
) {
    require(step == BootstrapStep.CreateIdentity || step == BootstrapStep.ImportIdentity)
    val title = if (step == BootstrapStep.CreateIdentity) "Create a local Nostr identity" else "Import an existing identity"
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        navigation = {
            HarvestCircleShellButton("Back", onBack, Modifier.testTag("identity-entry-back"))
        },
        header = { HarvestCircleShellText(title, role = HarvestCircleShellTextRole.PaneTitle) },
        body = {
            if (step == BootstrapStep.CreateIdentity) CreateIdentityBody(model) else ImportIdentityBody(model, actions)
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                if (step == BootstrapStep.CreateIdentity) {
                    HarvestCircleShellButton(
                        "Generate identity",
                        actions.generateIdentity,
                        Modifier.testTag("generate-key"),
                        primary = true,
                        enabled = !model.busy,
                    )
                } else {
                    HarvestCircleShellButton(
                        "Import identity",
                        actions.importSecretKey,
                        Modifier.testTag("import-key"),
                        primary = true,
                        enabled = !model.busy,
                    )
                }
                HarvestCircleShellButton("Back", onBack, Modifier.testTag("identity-entry-cancel"))
            }
        },
    )
}

@Composable
private fun CreateIdentityBody(model: HarvestCircleUiModel) {
    HarvestCircleShellPanel(Modifier.testTag("create-identity-entry")) {
        HarvestCircleShellText("HarvestCircle will generate a new Nostr identity.", role = HarvestCircleShellTextRole.SectionTitle)
        HarvestCircleShellText("Save the recovery key before the identity is stored in the operating-system keyring.")
        model.problem?.let { HarvestCircleShellText(it, Modifier.testTag("identity-entry-problem")) }
    }
}

@Composable
private fun ImportIdentityBody(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
) {
    val requester = remember { FocusRequester() }
    Column(Modifier.testTag("import-identity-entry"), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        HarvestCircleShellPanel {
            HarvestCircleShellTextField(
                value = model.importDraft.revealForDisplay(),
                onValueChange = actions.editImportDraft,
                label = "Nostr secret key",
                placeholder = "nsec1…",
                inputModifier =
                    Modifier
                        .focusRequester(requester)
                        .semantics {
                            password()
                            contentDescription = "Nostr secret key"
                        }.testTag("import-nsec-input"),
                visualTransformation = PasswordVisualTransformation(),
            )
            HarvestCircleShellText(
                "The secret is held only for this import.\n\n" +
                    "It is cleared after it is sent to the local native runtime.",
            )
        }
        model.importGuidance?.let { HarvestCircleShellText(it, Modifier.testTag("identity-entry-guidance")) }
        model.problem?.let { HarvestCircleShellText(it, Modifier.testTag("identity-entry-problem")) }
    }
    LaunchedEffect(Unit) { requester.requestFocus() }
}
