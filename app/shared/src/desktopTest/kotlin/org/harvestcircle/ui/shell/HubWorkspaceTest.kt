package org.harvestcircle.ui.shell

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.HubViewState
import org.harvestcircle.application.OperationId
import org.harvestcircle.navigation.AppRoute
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class HubWorkspaceTest {
    @Test
    fun setupIsExplicitAndSampleStatusIsVisible() =
        runComposeUiTest {
            var submitted = 0
            setHarvestCircleContent {
                HubWorkspace(AppRoute.HubWork, HubViewState.NeedsSetup, { submitted += 1 }, {})
            }
            onNodeWithText("Sample data only · One desktop · One operator").assertIsDisplayed()
            onNodeWithTag("hub-initialize").performClick()
            assertEquals(1, submitted)
        }

    @Test
    fun uncertainSetupOffersReadReconciliationAndKeepsOriginalId() =
        runComposeUiTest {
            val operation = OperationId.from("01900000-0000-7000-8000-000000000001")
            var reads = 0
            setHarvestCircleContent {
                HubWorkspace(AppRoute.HubStock, HubViewState.Failed("Reply unavailable", operation), {}, { reads += 1 })
            }
            onNodeWithText("Original setup operation: ${operation.value}").assertExists()
            onNodeWithTag("hub-initialize").assertDoesNotExist()
            onNodeWithTag("hub-refresh").performClick()
            assertEquals(1, reads)
        }

    @Test
    fun readOnlyCannotInitializeHub() =
        runComposeUiTest {
            setHarvestCircleContent { HubWorkspace(AppRoute.HubDispatch, HubViewState.SignedOut, {}, {}) }
            onNodeWithTag("hub-initialize").assertDoesNotExist()
            onNodeWithText("Activate the hub owner's local Nostr identity to access this workspace.").assertIsDisplayed()
        }
}
