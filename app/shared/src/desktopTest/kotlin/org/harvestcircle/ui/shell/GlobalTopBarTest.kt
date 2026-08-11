package org.harvestcircle.ui.shell

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.SignerStatusLabel
import org.harvestcircle.application.SyncStatusLabel
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class GlobalTopBarTest {
    @Test
    fun topBarUsesActualStatusAndTypedIntents() =
        runComposeUiTest {
            val intents = mutableListOf<GlobalTopBarIntent>()
            setContent {
                GlobalTopBar(
                    GlobalTopBarModel(
                        canGoBack = true,
                        canGoForward = false,
                        syncStatus = SyncStatusLabel.Degraded,
                        signerStatus = SignerStatusLabel.ReadOnly,
                    ),
                    intents::add,
                )
            }
            onNodeWithText("Limited connection").performClick()
            onNodeWithText("Read-only").performClick()
            onNodeWithTag("top-bar-open-reference").performClick()
            onNodeWithTag("top-bar-forward").assertIsNotEnabled()
            assertEquals(
                listOf(
                    GlobalTopBarIntent.ShowSyncStatus,
                    GlobalTopBarIntent.ShowSignerStatus,
                    GlobalTopBarIntent.OpenNostrReference,
                ),
                intents,
            )
        }
}
