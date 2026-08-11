package org.harvestcircle.ui.shell

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.BannerSeverity
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.GlobalStatusBanner
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.OverlayReducer
import org.harvestcircle.application.OverlayState
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class FoundationOverlayHostTest {
    @Test
    fun referenceDialogOwnsFocusAndReportsUnsupportedSyntax() =
        runComposeUiTest {
            var state by mutableStateOf(
                OverlayState(
                    current = FoundationOverlay.OpenNostrReference(),
                    banner = GlobalStatusBanner("Limited connection", BannerSeverity.Caution),
                ),
            )
            setContent { FoundationOverlayHost(state) { state = OverlayReducer.reduce(state, it) } }
            onAllNodesWithTag("foundation-overlay").assertCountEquals(1)
            onAllNodesWithTag("global-status-banner").assertCountEquals(1)
            onNodeWithTag("nostr-reference-input").assertIsFocused().performTextInput("note1qqqqqq")
            onNodeWithTag("nostr-reference-submit").performClick()
            onAllNodesWithTag("nostr-reference-result").assertCountEquals(1)
            state = OverlayReducer.reduce(state, OverlayIntent.Escape)
            onAllNodesWithTag("foundation-overlay").assertCountEquals(0)
        }
}
