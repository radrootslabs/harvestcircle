package org.harvestcircle.ui.shell

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class FoundationTodayScreenTest {
    @Test
    fun readOnlyEmptyStateIsTruthfulAndReferenceActionIsTyped() =
        runComposeUiTest {
            var opened = 0
            setHarvestCircleContent {
                FoundationTodayScreen(FoundationTodayModel("Read-only session")) { opened += 1 }
            }

            onNodeWithText("Read-only session").assertExists()
            onNodeWithText("No active commitments").assertExists()
            onNodeWithText("Explore nearby buying circles or open a shared Nostr reference.").assertExists()
            onNodeWithTag("today-explore-circles").assertIsNotEnabled()
            onNodeWithText("Not available in this build.").assertExists()
            onNodeWithTag("today-open-reference").performClick()
            assertEquals(1, opened)
        }

    @Test
    fun activeContextUsesTheActualIdentityLabel() =
        runComposeUiTest {
            setHarvestCircleContent { FoundationTodayScreen(FoundationTodayModel("Local grower"), openNostrReference = {}) }
            onNodeWithTag("today-context").assertTextEquals("Local grower")
        }
}
