package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPage
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellTab
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole

enum class NetworkIdentityState { ReadOnly, Active, CredentialUnavailable, Available, SignedOut }

enum class RelayObservationState { NotYetObserved, Available, Degraded, Unavailable }

enum class RelayCapability { Configured, NotConfigured }

enum class NetworkSection(
    val key: String,
    val label: String,
) {
    Overview("overview", "Overview"),
    Identity("identity", "Identity"),
    PublicRelays("public_relays", "Public relays"),
    Runtime("runtime", "Runtime"),
}

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
    section: NetworkSection = NetworkSection.Overview,
    onSectionSelected: (NetworkSection) -> Unit = {},
    showSectionTabs: Boolean = true,
) {
    var localSection by remember { mutableStateOf(section) }
    val selected = if (showSectionTabs) localSection else section
    HarvestCircleShellPage(Modifier.testTag("template-tabbed-detail")) {
        if (showSectionTabs) {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                NetworkSection.entries.forEach { candidate ->
                    HarvestCircleShellTab(
                        label = candidate.label,
                        selected = candidate == selected,
                        onClick = {
                            if (candidate != selected) {
                                localSection = candidate
                                onSectionSelected(candidate)
                            }
                        },
                        modifier = Modifier.testTag("network-tab-${candidate.key}"),
                    )
                }
            }
        }
        Box(
            Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .testTag("bounded-detail-network"),
        ) {
            NetworkDetail(selected, model, refreshProfile, signOut)
        }
    }
}

@Composable
private fun NetworkDetail(
    selection: NetworkSection,
    model: FoundationNetworkModel,
    refreshProfile: () -> Unit,
    signOut: () -> Unit,
) {
    Column(Modifier.testTag("network-${selection.key}"), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        when (selection) {
            NetworkSection.Overview -> {
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
                HarvestCircleShellText("No managed HarvestCircle service is configured.")
            }
            NetworkSection.Identity -> {
                HarvestCircleShellText(
                    model.identityState.label(),
                    Modifier.testTag("network-identity-state"),
                    HarvestCircleShellTextRole.Small,
                )
                model.identityLabel?.let {
                    HarvestCircleShellText(it, Modifier.testTag("network-identity-label"), HarvestCircleShellTextRole.SectionTitle)
                }
                model.profileLabel?.let { HarvestCircleShellText("Display name: $it", Modifier.testTag("network-profile-label")) }
                if (model.identityState == NetworkIdentityState.Active) {
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        HarvestCircleShellButton("Refresh profile", refreshProfile, Modifier.testTag("refresh-profile"), primary = true)
                        HarvestCircleShellButton("Sign out", signOut, Modifier.testTag("sign-out"))
                    }
                }
            }
            NetworkSection.PublicRelays -> {
                HarvestCircleShellText(
                    model.relayState.label(),
                    Modifier.testTag("network-relay-state"),
                    HarvestCircleShellTextRole.Small,
                )
                if (model.relays.isEmpty()) {
                    HarvestCircleShellText("No public relay endpoints are configured.", Modifier.testTag("network-relays-empty"))
                }
                model.relays.forEach { relay ->
                    HarvestCircleShellPanel(Modifier.testTag("network-relay:${relay.url}")) {
                        HarvestCircleShellText(relay.url, role = HarvestCircleShellTextRole.Code)
                        HarvestCircleShellText(relay.destination.label())
                        HarvestCircleShellText(relay.readCapability.label("Read"))
                        HarvestCircleShellText(relay.writeCapability.label("Write"))
                    }
                }
            }
            NetworkSection.Runtime -> {
                Fact("Local runtime", model.runtimeState.label())
                model.runtimeProblem?.let { HarvestCircleShellText(it, Modifier.testTag("network-runtime-problem")) }
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
        HarvestCircleShellText(
            label,
            role = HarvestCircleShellTextRole.Small,
            color = org.harvestcircle.designsystem.shell.HarvestCircleShellPalette.contentSecondary,
        )
        HarvestCircleShellText(value, Modifier.testTag("network-fact-${label.lowercase().replace(' ', '-')}"))
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
