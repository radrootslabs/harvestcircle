package org.harvestcircle.ui.shell

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.BannerSeverity
import org.harvestcircle.application.ConfirmationAction
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.GlobalStatusBanner
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.OverlayReducer
import org.harvestcircle.application.OverlayState
import org.harvestcircle.application.ReferenceResult
import org.harvestcircle.application.ShellStatusModel
import org.harvestcircle.application.SignerStatusLabel
import org.harvestcircle.application.SyncStatusLabel
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class FoundationOverlayHostTest {
    @Test
    fun referenceDialogOwnsFocusAndReportsUnsupportedSyntax() =
        runComposeUiTest {
            var state by mutableStateOf(
                OverlayState(
                    current = FoundationOverlay.OpenNostrReference(),
                ),
            )
            setContent {
                FoundationOverlayHost(
                    state,
                    status(banner = GlobalStatusBanner("Limited connection", "Some services are unavailable.", BannerSeverity.Caution)),
                ) {
                    state =
                        OverlayReducer.reduce(
                            state,
                            if (it == OverlayIntent.SubmitReference) {
                                OverlayIntent.ApplyReferenceResult(ReferenceResult.Invalid)
                            } else {
                                it
                            },
                        )
                }
            }
            onAllNodesWithTag("foundation-overlay").assertCountEquals(1)
            onAllNodesWithTag("global-status-banner").assertCountEquals(1)
            onNodeWithTag("nostr-reference-input").assertIsFocused().performTextInput("note1qqqqqq")
            onNodeWithTag("nostr-reference-submit").performClick()
            onAllNodesWithTag("nostr-reference-result").assertCountEquals(1)
            state = OverlayReducer.reduce(state, OverlayIntent.Escape)
            onAllNodesWithTag("foundation-overlay").assertCountEquals(0)
        }

    @Test
    fun destructiveConfirmationOwnsFocusAndDispatchesTypedSubmission() =
        runComposeUiTest {
            val intents = mutableListOf<OverlayIntent>()
            val state =
                OverlayState(
                    current =
                        FoundationOverlay.ConfirmAction(
                            "Remove identity?",
                            "The local credential will be deleted.",
                            "Remove local identity",
                            ConfirmationAction.RemoveLocalIdentity,
                        ),
                )
            setContent { FoundationOverlayHost(state, status(), onIntent = intents::add) }

            onNodeWithTag("overlay-confirm").assertIsFocused().performClick()
            kotlin.test.assertEquals(listOf<OverlayIntent>(OverlayIntent.Confirm), intents)
        }

    @Test
    fun busyConfirmationBlocksDuplicateSubmissionAndDismissal() =
        runComposeUiTest {
            val intents = mutableListOf<OverlayIntent>()
            val state =
                OverlayState(
                    current =
                        FoundationOverlay.ConfirmAction(
                            "Remove identity?",
                            "The local credential will be deleted.",
                            "Remove local identity",
                            ConfirmationAction.RemoveLocalIdentity,
                        ),
                )
            setContent { FoundationOverlayHost(state, status(), busy = true, onIntent = intents::add) }

            onNodeWithTag("overlay-confirm").assertIsNotEnabled().performClick()
            onNodeWithTag("overlay-cancel").assertIsNotEnabled()
            kotlin.test.assertTrue(intents.isEmpty())
        }

    @Test
    fun openStatusDialogRendersTheLatestStatusModel() =
        runComposeUiTest {
            var status by mutableStateOf(status())
            val overlay = OverlayState(FoundationOverlay.Status(org.harvestcircle.application.StatusOverlayKey.Sync))
            setContent { FoundationOverlayHost(overlay, status, onIntent = {}) }

            onNodeWithText("Not yet observed").assertExists()
            status = ShellStatusModel(SyncStatusLabel.Degraded, SignerStatusLabel.SignedOut, null)
            onNodeWithText("Limited connection").assertExists()
        }
}

private fun status(banner: GlobalStatusBanner? = null) =
    ShellStatusModel(SyncStatusLabel.NotYetObserved, SignerStatusLabel.SignedOut, banner)
