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
import org.harvestcircle.design.TextSizePreference
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
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        navigation = { ShellAction("Back", "Back", "identity-entry-back", onClick = onBack) },
        header = {
            ShellText(
                if (step == BootstrapStep.CreateIdentity) {
                    "Create a local Nostr identity"
                } else {
                    "Import an existing identity"
                },
                textRole = ShellTextRole.ScreenTitle,
            )
        },
        body = {
            if (step == BootstrapStep.CreateIdentity) {
                CreateIdentityBody(model)
            } else {
                ImportIdentityBody(model, actions)
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                if (step == BootstrapStep.CreateIdentity) {
                    ShellAction("Generate identity", "Generate identity", "generate-key", enabled = !model.busy) {
                        actions.generateIdentity()
                    }
                } else {
                    ShellAction("Import identity", "Import identity", "import-key", enabled = !model.busy) {
                        actions.importSecretKey()
                    }
                }
                ShellAction("Back", "Back", "identity-entry-cancel", onClick = onBack)
            }
        },
    )
}

@Composable
private fun CreateIdentityBody(model: HarvestCircleUiModel) {
    Column(Modifier.testTag("create-identity-entry"), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        ShellText("HarvestCircle will generate a new Nostr identity.")
        ShellText("Save the recovery key before the identity is stored in the operating-system keyring.")
        model.problem?.let { ShellText(it, Modifier.testTag("identity-entry-problem")) }
    }
}

@Composable
private fun ImportIdentityBody(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
) {
    val requester = remember { FocusRequester() }
    Column(Modifier.testTag("import-identity-entry"), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        ShellTextField(
            value = model.importDraft,
            onValueChange = actions.editImportDraft,
            label = "Nostr secret key",
            placeholder = "nsec1…",
            modifier =
                Modifier
                    .focusRequester(requester)
                    .semantics {
                        password()
                        contentDescription = "Nostr secret key"
                    }.testTag("import-nsec-input"),
            visualTransformation = PasswordVisualTransformation(),
        )
        ShellText("The secret is sent directly to the local native runtime and is not retained in the interface.")
        model.importGuidance?.let { ShellText(it, Modifier.testTag("identity-entry-guidance")) }
        model.problem?.let { ShellText(it, Modifier.testTag("identity-entry-problem")) }
    }
    LaunchedEffect(Unit) { requester.requestFocus() }
}
