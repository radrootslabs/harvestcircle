package org.harvestcircle.ui.shell

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.BannerSeverity
import org.harvestcircle.application.ConfirmationAction
import org.harvestcircle.application.ConfirmationPhase
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
            setHarvestCircleContent {
                FoundationOverlayHost(
                    state,
                    status(banner = GlobalStatusBanner("Limited connection", "Some services are unavailable.", BannerSeverity.Caution)),
                ) {
                    state =
                        reduceOverlay(
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
            onNode(isDialog()).assertExists()
            onAllNodesWithText("Open a Nostr reference").assertCountEquals(2)
            onNodeWithText("Nostr link, note1, nevent1, or address").assertExists()
            onNodeWithText("nostr:…").assertExists()
            onAllNodesWithTag("global-status-banner").assertCountEquals(1)
            onNodeWithTag("nostr-reference-input").assertIsFocused().performTextInput("note1qqqqqq")
            onNodeWithTag("nostr-reference-submit").performClick()
            onAllNodesWithTag("nostr-reference-result").assertCountEquals(1)
            state = reduceOverlay(state, OverlayIntent.Escape())
            onAllNodesWithTag("foundation-overlay").assertCountEquals(0)
        }

    @Test
    fun destructiveConfirmationOwnsFocusAndDispatchesTypedSubmission() =
        runComposeUiTest {
            val intents = mutableListOf<OverlayIntent>()
            val action = removalAction()
            val state =
                OverlayState(
                    current =
                        FoundationOverlay.ConfirmAction(
                            "Remove identity?",
                            "The local credential will be deleted.",
                            "Remove local identity",
                            action,
                        ),
                )
            setHarvestCircleContent { FoundationOverlayHost(state, status(), onIntent = intents::add) }

            onNodeWithTag("overlay-confirm").assertIsFocused().performClick()
            kotlin.test.assertEquals(listOf<OverlayIntent>(OverlayIntent.Confirm(action)), intents)
        }

    @Test
    fun confirmationFocusWrapsInBothDirections() =
        runComposeUiTest {
            val state =
                OverlayState(
                    current =
                        FoundationOverlay.ConfirmAction(
                            "Remove identity?",
                            "The local credential will be deleted.",
                            "Remove local identity",
                            removalAction(),
                        ),
                )
            setHarvestCircleContent { FoundationOverlayHost(state, status(), onIntent = {}) }

            onNodeWithTag("overlay-confirm").assertIsFocused().pressTab()
            onNodeWithTag("overlay-cancel").assertIsFocused().pressTab()
            onNodeWithTag("overlay-confirm").assertIsFocused().pressShiftTab()
            onNodeWithTag("overlay-cancel").assertIsFocused()
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
                            removalAction(),
                            phase = ConfirmationPhase.Submitting,
                        ),
                )
            setHarvestCircleContent { FoundationOverlayHost(state, status(), onIntent = intents::add) }

            onNodeWithTag("foundation-overlay").assertIsFocused()
            onNodeWithTag("foundation-overlay").performKeyInput {
                keyDown(Key.Escape)
                keyUp(Key.Escape)
            }
            onNodeWithTag("overlay-confirm").assertIsNotEnabled().performClick()
            onNodeWithTag("overlay-cancel").assertIsNotEnabled()
            onNodeWithTag("foundation-overlay").assertExists()
            kotlin.test.assertTrue(intents.isEmpty())
        }

    @Test
    fun hcSl006ReferenceAndStatusControlsRemainLocallyInteractive() =
        runComposeUiTest {
            var state by mutableStateOf(OverlayState(current = FoundationOverlay.OpenNostrReference()))
            setHarvestCircleContent {
                FoundationOverlayHost(state, status()) { intent ->
                    state = reduceOverlay(state, intent)
                }
            }

            onNodeWithTag("nostr-reference-input").assertIsEnabled().performTextInput("note1candidate")
            onNodeWithTag("nostr-reference-submit").assertIsEnabled()
            onNodeWithTag("overlay-cancel").assertIsEnabled().performClick()
            onAllNodesWithTag("foundation-overlay").assertCountEquals(0)

            state = OverlayState(FoundationOverlay.Status(org.harvestcircle.application.StatusOverlayKey.Sync))
            onNodeWithTag("overlay-close").assertIsEnabled().performClick()
            onAllNodesWithTag("foundation-overlay").assertCountEquals(0)
        }

    @Test
    fun referenceFocusWrapsAcrossInputAndActions() =
        runComposeUiTest {
            val state = OverlayState(current = FoundationOverlay.OpenNostrReference())
            setHarvestCircleContent { FoundationOverlayHost(state, status(), onIntent = {}) }

            onNodeWithTag("nostr-reference-input").assertIsFocused().pressTab()
            onNodeWithTag("nostr-reference-submit").assertIsFocused().pressTab()
            onNodeWithTag("overlay-cancel").assertIsFocused().pressTab()
            onNodeWithTag("nostr-reference-input").assertIsFocused().pressShiftTab()
            onNodeWithTag("overlay-cancel").assertIsFocused()
        }

    @Test
    fun statusFocusRemainsContainedOnTab() =
        runComposeUiTest {
            val state = OverlayState(FoundationOverlay.Status(org.harvestcircle.application.StatusOverlayKey.Sync))
            setHarvestCircleContent { FoundationOverlayHost(state, status(), onIntent = {}) }

            onNodeWithTag("overlay-close").assertIsFocused().pressTab()
            onNodeWithTag("overlay-close").assertIsFocused().pressShiftTab()
            onNodeWithTag("overlay-close").assertIsFocused()
        }

    @Test
    fun openStatusDialogRendersTheLatestStatusModel() =
        runComposeUiTest {
            var status by mutableStateOf(status())
            val overlay = OverlayState(FoundationOverlay.Status(org.harvestcircle.application.StatusOverlayKey.Sync))
            setHarvestCircleContent { FoundationOverlayHost(overlay, status, onIntent = {}) }

            onNodeWithText("Not yet observed").assertExists()
            status = ShellStatusModel(SyncStatusLabel.Degraded, SignerStatusLabel.SignedOut, null)
            onNodeWithText("Limited connection").assertExists()
        }
}

private fun androidx.compose.ui.test.SemanticsNodeInteraction.pressTab() {
    performKeyInput {
        keyDown(Key.Tab)
        keyUp(Key.Tab)
    }
}

private fun androidx.compose.ui.test.SemanticsNodeInteraction.pressShiftTab() {
    performKeyInput {
        keyDown(Key.ShiftLeft)
        keyDown(Key.Tab)
        keyUp(Key.Tab)
        keyUp(Key.ShiftLeft)
    }
}

private fun status(banner: GlobalStatusBanner? = null) =
    ShellStatusModel(SyncStatusLabel.NotYetObserved, SignerStatusLabel.SignedOut, banner)

private fun removalAction() =
    ConfirmationAction.RemoveLocalIdentity(
        org.harvestcircle.application.IdentityId
            .fromPublicKeyHex("05".repeat(32)),
        org.harvestcircle.application.RemovalRequestId
            .from("overlay-removal"),
    )

private fun reduceOverlay(
    overlays: OverlayState,
    intent: OverlayIntent,
): OverlayState {
    val identity =
        org.harvestcircle.application.HarvestCirclePresenterState(
            org.harvestcircle.application.ApplicationSnapshot(
                revision = org.harvestcircle.application.SnapshotRevision(1UL),
                lifecycle = org.harvestcircle.application.ApplicationLifecycle.Ready,
                lifecycleProblem = null,
                configuredRelays = emptyList(),
                identities = emptyList(),
                selectedIdentityId = null,
                session = org.harvestcircle.application.SessionLifecycle.SignedOut,
                sessionSubjectIdentityId = null,
                sessionProblem = null,
                activeIdentity = null,
                recoverableProblem = null,
            ),
        )
    val shell =
        org.harvestcircle.application.HarvestCircleShellState(
            identity,
            org.harvestcircle.application.BuildInfo
                .unknown(),
            overlays = overlays,
        )
    return OverlayReducer.transition(shell, intent).state.overlays
}
