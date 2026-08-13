package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.application.RelayDestination
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class FoundationNetworkScreenTest {
    @Test
    fun overviewIsTruthfulAndClaimsNoManagedService() =
        runComposeUiTest {
            setHarvestCircleContent { FoundationNetworkScreen(model()) }
            onNodeWithTag("bounded-detail-network").assertExists()
            onNodeWithTag("network-tab-overview").assertIsSelected().assertIsEnabled().performClick()
            onNodeWithTag("network-tab-overview").assertIsSelected()
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
            setHarvestCircleContent {
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
            onNodeWithText("Read configured").assertExists()
            onNodeWithText("Write not configured").assertExists()
            onNodeWithText("Degraded").assertExists()
            onNodeWithTag("network-tab-runtime").performClick()
            onNodeWithText("Storage is temporarily unavailable.").assertExists()
        }

    @Test
    fun signedOutAndReadOnlyStatesRemainExplicit() =
        runComposeUiTest {
            setHarvestCircleContent { FoundationNetworkScreen(model(identityState = NetworkIdentityState.ReadOnly, relays = emptyList())) }
            onNodeWithText("Read-only").assertExists()
            onNodeWithTag("network-tab-public_relays").performClick()
            onNodeWithText("Not yet observed").assertExists()
            onNodeWithText("No public relay endpoints are configured.").assertExists()
        }

    @Test
    fun veryLargeRuntimeDetailsRemainReachableInTheBoundedPane() =
        runComposeUiTest {
            setHarvestCircleContent {
                Box(Modifier.size(640.dp, 360.dp)) {
                    HarvestCircleTheme(AppearanceState(textSize = TextSizePreference.VeryLarge)) {
                        FoundationNetworkScreen(model())
                    }
                }
            }

            onNodeWithTag("network-tab-runtime").performClick()
            onNodeWithTag("network-runtime-problem").performScrollTo().assertIsDisplayed()
        }
}

private fun model(
    identityState: NetworkIdentityState = NetworkIdentityState.Active,
    relays: List<NetworkRelayModel> =
        listOf(
            NetworkRelayModel(
                url = "wss://relay.example",
                destination = RelayDestination.Public,
                readCapability = RelayCapability.Configured,
                writeCapability = RelayCapability.NotConfigured,
            ),
        ),
) = FoundationNetworkModel(
    identityState = identityState,
    identityLabel = "Grower identity",
    profileLabel = "Farm Identity",
    relayState = if (relays.isEmpty()) RelayObservationState.NotYetObserved else RelayObservationState.Degraded,
    relays = relays,
    runtimeState = ApplicationLifecycle.Degraded,
    runtimeProblem = "Storage is temporarily unavailable.",
)
