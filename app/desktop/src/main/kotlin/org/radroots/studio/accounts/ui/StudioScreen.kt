package org.radroots.studio.accounts.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp

private val WindowBackgroundColor = Color(0xFFF5F5F2)
private val ButtonBackgroundColor = Color(0xFFE7E7E2)
private val InputBackgroundColor = Color(0xFFFEFDF8)

data class StudioUiActions(
    val editImportDraft: (String) -> Unit = {},
    val generateAccount: () -> Unit = {},
    val importSecretKey: () -> Unit = {},
)

@Composable
fun StudioScreen(
    model: StudioUiModel,
    actions: StudioUiActions,
) {
    InactiveAccountsScreen(model, actions)
}

@Composable
private fun InactiveAccountsScreen(
    model: StudioUiModel,
    actions: StudioUiActions,
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

        TextAction(
            text = "Generate new key",
            testTag = "generate-key",
            contentDescription = "Generate a new Nostr key",
            enabled = !model.busy,
            onClick = actions.generateAccount,
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

        model.problem?.let {
            BasicText(it, Modifier.testTag("accounts-problem"))
        }

        if (model.accounts.isEmpty()) {
            BasicText("No saved accounts.", Modifier.testTag("accounts-empty"))
        }
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
