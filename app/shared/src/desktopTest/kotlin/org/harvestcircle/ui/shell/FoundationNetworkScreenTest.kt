package org.harvestcircle.ui.shell

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class FoundationNetworkScreenTest {
    @Test
    fun overviewIsTruthfulAndClaimsNoManagedService() =
        runComposeUiTest {
            setContent { FoundationNetworkScreen(model()) }
            onNodeWithText("Signer").assertExists()
            onNodeWithText("Local identity active").assertExists()
            onNodeWithText("No managed HarvestCircle service is configured.").assertExists()
            onAllNodesWithText("Authority").assertCountEquals(0)
            onAllNodesWithText("Inbox").assertCountEquals(0)
        }

    @Test
    fun identityRelaysAndRuntimeUseOnlyTheSuppliedFoundationState() =
        runComposeUiTest {
            var refreshed = 0
            var signedOut = 0
            setContent {
                FoundationNetworkScreen(
                    model(),
                    refreshProfile = { refreshed += 1 },
                    signOut = { signedOut += 1 },
                )
            }
            onNodeWithTag("network-tab-identity").performClick()
            onNodeWithText("Grower identity").assertExists()
            onNodeWithTag("refresh-profile").performClick()
            onNodeWithTag("sign-out").performClick()
            kotlin.test.assertEquals(1, refreshed)
            kotlin.test.assertEquals(1, signedOut)
            onNodeWithTag("network-tab-public_relays").performClick()
            onNodeWithText("wss://relay.example").assertExists()
            onNodeWithText("Public").assertExists()
            onNodeWithText("Read available").assertExists()
            onNodeWithText("Write unavailable").assertExists()
            onNodeWithText("Degraded").assertExists()
            onNodeWithTag("network-tab-runtime").performClick()
            onNodeWithText("Storage is temporarily unavailable.").assertExists()
        }

    @Test
    fun signedOutAndReadOnlyStatesRemainExplicit() =
        runComposeUiTest {
            setContent { FoundationNetworkScreen(model(identityState = "Read-only", relays = emptyList())) }
            onNodeWithText("Read-only").assertExists()
            onNodeWithTag("network-tab-public_relays").performClick()
            onNodeWithText("Not yet observed").assertExists()
        }
}

private fun model(
    identityState: String = "Local identity active",
    relays: List<NetworkRelayModel> =
        listOf(
            NetworkRelayModel(
                url = "wss://relay.example",
                destination = "Public",
                readState = "Read available",
                writeState = "Write unavailable",
            ),
        ),
) = FoundationNetworkModel(
    identityState = identityState,
    identityLabel = "Grower identity",
    profileLabel = "Farm Identity",
    relayState = if (relays.isEmpty()) "Not yet observed" else "Degraded",
    relays = relays,
    runtimeState = "Degraded",
    runtimeProblem = "Storage is temporarily unavailable.",
    pendingOperations = 1,
)
