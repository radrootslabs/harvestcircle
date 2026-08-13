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
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.action.HarvestCircleLabeledButton
import org.harvestcircle.designsystem.component.input.HarvestCircleTextField
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
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
        navigation = {
            HarvestCircleLabeledButton("Back", "Back", onBack, Modifier.testTag("identity-entry-back"))
        },
        header = {
            HarvestCircleText(
                if (step == BootstrapStep.CreateIdentity) {
                    "Create a local Nostr identity"
                } else {
                    "Import an existing identity"
                },
                role = HarvestCircleTextRole.PageTitle,
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
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                if (step == BootstrapStep.CreateIdentity) {
                    HarvestCircleLabeledButton(
                        "Generate identity",
                        "Generate identity",
                        { actions.generateIdentity() },
                        Modifier.testTag("generate-key"),
                        enabled = !model.busy,
                    )
                } else {
                    HarvestCircleLabeledButton(
                        "Import identity",
                        "Import identity",
                        { actions.importSecretKey() },
                        Modifier.testTag("import-key"),
                        enabled = !model.busy,
                    )
                }
                HarvestCircleLabeledButton("Back", "Back", onBack, Modifier.testTag("identity-entry-cancel"))
            }
        },
    )
}

@Composable
private fun CreateIdentityBody(model: HarvestCircleUiModel) {
    Column(
        Modifier.testTag("create-identity-entry"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleText("HarvestCircle will generate a new Nostr identity.")
        HarvestCircleText("Save the recovery key before the identity is stored in the operating-system keyring.")
        model.problem?.let { HarvestCircleText(it, Modifier.testTag("identity-entry-problem")) }
    }
}

@Composable
private fun ImportIdentityBody(
    model: HarvestCircleUiModel,
    actions: HarvestCircleUiActions,
) {
    val requester = remember { FocusRequester() }
    Column(
        Modifier.testTag("import-identity-entry"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleTextField(
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
        HarvestCircleText(
            "The secret is held only for this import.\n\n" +
                "It is cleared after it is sent to the local native runtime.",
        )
        model.importGuidance?.let { HarvestCircleText(it, Modifier.testTag("identity-entry-guidance")) }
        model.problem?.let { HarvestCircleText(it, Modifier.testTag("identity-entry-problem")) }
    }
    LaunchedEffect(Unit) { requester.requestFocus() }
}
