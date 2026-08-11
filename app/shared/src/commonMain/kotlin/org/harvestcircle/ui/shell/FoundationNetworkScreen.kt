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
import org.harvestcircle.application.RelayDestination
import org.harvestcircle.application.SignerAvailability

enum class NetworkIdentityState { ReadOnly, Active, CredentialUnavailable, Available, SignedOut }

enum class RelayObservationState { NotYetObserved, Available, Degraded, Unavailable }

enum class RelayCapability { Configured, NotConfigured }

data class NetworkRelayModel(
    val url: String,
    val destination: RelayDestination,
    val readCapability: RelayCapability,
    val writeCapability: RelayCapability,
)

data class FoundationNetworkModel(
    val identityState: NetworkIdentityState,
    val identityLabel: String?,
    val profileLabel: String?,
    val relayState: RelayObservationState,
    val relays: List<NetworkRelayModel>,
    val runtimeState: ApplicationLifecycle,
    val runtimeProblem: String?,
)

fun foundationNetworkModel(state: HarvestCircleShellState): FoundationNetworkModel {
    val snapshot = state.identity.snapshot
    val active = snapshot.activeIdentity
    val identityState =
        when {
            state.session.readOnly -> NetworkIdentityState.ReadOnly
            active?.identity?.signer?.availability == SignerAvailability.Available -> NetworkIdentityState.Active
            active != null -> NetworkIdentityState.CredentialUnavailable
            snapshot.identities.isNotEmpty() -> NetworkIdentityState.Available
            else -> NetworkIdentityState.SignedOut
        }
    return FoundationNetworkModel(
        identityState = identityState,
        identityLabel = active?.identity?.displayLabel ?: snapshot.identities.firstOrNull()?.displayLabel,
        profileLabel = active?.profile?.displayName ?: active?.profile?.name,
        relayState = active?.relays?.state?.toObservation() ?: RelayObservationState.NotYetObserved,
        relays =
            snapshot.configuredRelays.map { relay ->
                NetworkRelayModel(
                    url = relay.url,
                    destination = relay.destination,
                    readCapability = relay.read.toCapability(),
                    writeCapability = relay.write.toCapability(),
                )
            },
        runtimeState = snapshot.lifecycle,
        runtimeProblem =
            snapshot.lifecycleProblem?.safeMessage
                ?: snapshot.sessionProblem?.safeMessage
                ?: snapshot.recoverableProblem?.safeMessage
                ?: state.identity.problem,
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
                Fact("Signer", model.identityState.label())
                Fact(
                    "Configured read endpoints",
                    model.relays.count { it.readCapability == RelayCapability.Configured }.toString(),
                )
                Fact(
                    "Configured write endpoints",
                    model.relays.count { it.writeCapability == RelayCapability.Configured }.toString(),
                )
                Fact("Local runtime", model.runtimeState.label())
                BasicText("No managed HarvestCircle service is configured.")
            }
            "identity" -> {
                BasicText(model.identityState.label(), Modifier.testTag("network-identity-state"))
                model.identityLabel?.let { BasicText(it, Modifier.testTag("network-identity-label")) }
                model.profileLabel?.let { BasicText("Display name: $it", Modifier.testTag("network-profile-label")) }
                if (model.identityState == NetworkIdentityState.Active) {
                    ShellAction("Refresh profile", "Refresh active profile", "refresh-profile", onClick = refreshProfile)
                    ShellAction("Sign out", "Sign out", "sign-out", onClick = signOut)
                }
            }
            "public_relays" -> {
                BasicText(model.relayState.label(), Modifier.testTag("network-relay-state"))
                if (model.relays.isEmpty()) {
                    BasicText("No public relay endpoints are configured.", Modifier.testTag("network-relays-empty"))
                }
                model.relays.forEach { relay ->
                    Column(Modifier.testTag("network-relay:${relay.url}")) {
                        BasicText(relay.url)
                        BasicText(relay.destination.label())
                        BasicText(relay.readCapability.label("Read"))
                        BasicText(relay.writeCapability.label("Write"))
                    }
                }
            }
            "runtime" -> {
                Fact("Local runtime", model.runtimeState.label())
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

private fun RelayConnectionState.toObservation(): RelayObservationState =
    when (this) {
        RelayConnectionState.Disconnected, RelayConnectionState.Error -> RelayObservationState.Unavailable
        RelayConnectionState.Connecting -> RelayObservationState.NotYetObserved
        RelayConnectionState.Connected -> RelayObservationState.Available
        RelayConnectionState.Degraded -> RelayObservationState.Degraded
    }

private fun NetworkIdentityState.label(): String =
    when (this) {
        NetworkIdentityState.ReadOnly -> "Read-only"
        NetworkIdentityState.Active -> "Local identity active"
        NetworkIdentityState.CredentialUnavailable -> "Credential unavailable"
        NetworkIdentityState.Available -> "Local identity available"
        NetworkIdentityState.SignedOut -> "Signed out"
    }

private fun RelayObservationState.label(): String =
    when (this) {
        RelayObservationState.NotYetObserved -> "Not yet observed"
        RelayObservationState.Available -> "Available"
        RelayObservationState.Degraded -> "Degraded"
        RelayObservationState.Unavailable -> "Unavailable"
    }

private fun RelayDestination.label(): String = name

private fun Boolean.toCapability(): RelayCapability = if (this) RelayCapability.Configured else RelayCapability.NotConfigured

private fun RelayCapability.label(direction: String): String =
    when (this) {
        RelayCapability.Configured -> "$direction configured"
        RelayCapability.NotConfigured -> "$direction not configured"
    }

private fun ApplicationLifecycle.label(): String =
    when (this) {
        ApplicationLifecycle.Ready -> "Available"
        ApplicationLifecycle.Degraded -> "Degraded"
        ApplicationLifecycle.Blocked, ApplicationLifecycle.Fatal, ApplicationLifecycle.Closed -> "Unavailable"
        else -> "Not yet observed"
    }
