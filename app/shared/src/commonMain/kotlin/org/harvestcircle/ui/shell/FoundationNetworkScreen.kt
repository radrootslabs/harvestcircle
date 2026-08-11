package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.application.HarvestCircleShellState
import org.harvestcircle.application.RelayConnectionState
import org.harvestcircle.application.SignerAvailability

data class NetworkRelayModel(
    val url: String,
    val destination: String,
    val readState: String,
    val writeState: String,
)

data class FoundationNetworkModel(
    val identityState: String,
    val identityLabel: String?,
    val profileLabel: String?,
    val relayState: String,
    val relays: List<NetworkRelayModel>,
    val runtimeState: String,
    val runtimeProblem: String?,
    val pendingOperations: Int,
)

fun foundationNetworkModel(state: HarvestCircleShellState): FoundationNetworkModel {
    val snapshot = state.identity.snapshot
    val active = snapshot.activeIdentity
    val identityState =
        when {
            state.session.readOnly -> "Read-only"
            active?.identity?.signer?.availability == SignerAvailability.Available -> "Local identity active"
            active != null -> "Credential unavailable"
            snapshot.identities.isNotEmpty() -> "Local identity available"
            else -> "Signed out"
        }
    return FoundationNetworkModel(
        identityState = identityState,
        identityLabel = active?.identity?.displayLabel ?: snapshot.identities.firstOrNull()?.displayLabel,
        profileLabel = active?.profile?.displayName ?: active?.profile?.name,
        relayState = active?.relays?.state?.label() ?: "Not yet observed",
        relays =
            snapshot.configuredRelays.map { relay ->
                NetworkRelayModel(
                    url = relay.url,
                    destination = relay.destination.name,
                    readState = if (relay.read) "Read available" else "Read unavailable",
                    writeState = if (relay.write) "Write available" else "Write unavailable",
                )
            },
        runtimeState = snapshot.lifecycle.label(),
        runtimeProblem =
            snapshot.lifecycleProblem?.safeMessage
                ?: snapshot.sessionProblem?.safeMessage
                ?: snapshot.recoverableProblem?.safeMessage
                ?: state.identity.problem,
        pendingOperations = if (state.identity.busy) 1 else 0,
    )
}

@Composable
fun FoundationNetworkScreen(
    model: FoundationNetworkModel,
    refreshProfile: () -> Unit = {},
    signOut: () -> Unit = {},
) {
    val tabs =
        listOf(
            TemplateTab(TemplateSelectionKey("overview"), "Overview"),
            TemplateTab(TemplateSelectionKey("identity"), "Identity"),
            TemplateTab(TemplateSelectionKey("public_relays"), "Public relays"),
            TemplateTab(TemplateSelectionKey("runtime"), "Runtime"),
        )
    var selected by remember { mutableStateOf(tabs.first().key) }
    TabbedDetailTemplate(
        tabs = tabs,
        selected = selected,
        tabRail = { available, current ->
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                available.forEach { tab ->
                    ShellAction(
                        label = tab.label,
                        description = "Show ${tab.label}",
                        tag = "network-tab-${tab.key.value}",
                        enabled = tab.key != current,
                    ) { selected = tab.key }
                }
            }
        },
        detail = { selection -> NetworkDetail(selection, model, refreshProfile, signOut) },
    )
}

@Composable
private fun NetworkDetail(
    selection: TemplateSelectionKey,
    model: FoundationNetworkModel,
    refreshProfile: () -> Unit,
    signOut: () -> Unit,
) {
    Column(
        Modifier.testTag("network-${selection.value}"),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        when (selection.value) {
            "overview" -> {
                Fact("Signer", model.identityState)
                Fact("Public relay reads", model.relays.count { it.readState == "Read available" }.toString())
                Fact("Public relay writes", model.relays.count { it.writeState == "Write available" }.toString())
                Fact("Local runtime", model.runtimeState)
                Fact("Pending operations", model.pendingOperations.toString())
                BasicText("No managed HarvestCircle service is configured.")
            }
            "identity" -> {
                BasicText(model.identityState, Modifier.testTag("network-identity-state"))
                model.identityLabel?.let { BasicText(it, Modifier.testTag("network-identity-label")) }
                model.profileLabel?.let { BasicText("Display name: $it", Modifier.testTag("network-profile-label")) }
                if (model.identityState == "Local identity active") {
                    ShellAction("Refresh profile", "Refresh active profile", "refresh-profile", onClick = refreshProfile)
                    ShellAction("Sign out", "Sign out", "sign-out", onClick = signOut)
                }
            }
            "public_relays" -> {
                BasicText(model.relayState, Modifier.testTag("network-relay-state"))
                model.relays.forEach { relay ->
                    Column(Modifier.testTag("network-relay:${relay.url}")) {
                        BasicText(relay.url)
                        BasicText(relay.destination)
                        BasicText(relay.readState)
                        BasicText(relay.writeState)
                    }
                }
            }
            "runtime" -> {
                Fact("Local runtime", model.runtimeState)
                Fact("Pending operations", model.pendingOperations.toString())
                model.runtimeProblem?.let { BasicText(it, Modifier.testTag("network-runtime-problem")) }
            }
        }
    }
}

@Composable
private fun Fact(
    label: String,
    value: String,
) {
    Column {
        BasicText(label)
        BasicText(value, Modifier.testTag("network-fact-${label.lowercase().replace(' ', '-') }"))
    }
}

private fun RelayConnectionState.label(): String =
    when (this) {
        RelayConnectionState.Disconnected -> "Unavailable"
        RelayConnectionState.Connecting -> "Not yet observed"
        RelayConnectionState.Connected -> "Available"
        RelayConnectionState.Degraded -> "Degraded"
        RelayConnectionState.Error -> "Unavailable"
    }

private fun ApplicationLifecycle.label(): String =
    when (this) {
        ApplicationLifecycle.Ready -> "Available"
        ApplicationLifecycle.Degraded -> "Degraded"
        ApplicationLifecycle.Blocked, ApplicationLifecycle.Fatal, ApplicationLifecycle.Closed -> "Unavailable"
        else -> "Not yet observed"
    }
