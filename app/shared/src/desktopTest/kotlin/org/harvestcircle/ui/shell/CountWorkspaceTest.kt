package org.harvestcircle.ui.shell

import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.CountAttempt
import org.harvestcircle.application.CountContext
import org.harvestcircle.application.CountDraft
import org.harvestcircle.application.CountFields
import org.harvestcircle.application.CountPresenter
import org.harvestcircle.application.CountRequest
import org.harvestcircle.application.CountSubmission
import org.harvestcircle.application.CountViewState
import org.harvestcircle.application.HandlingFixtures
import org.harvestcircle.application.HubCountObserveInput
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.StockFixtures
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class CountWorkspaceTest {
    @Test
    fun staleRetainedConfirmationCannotEnableSubmissionAndPhysicalContextRemainsVisible() =
        runComposeUiTest {
            setHarvestCircleContent {
                CountWorkspace(
                    CountViewState(
                        available = true,
                        open = true,
                        needsReview = true,
                        draft =
                            CountDraft(
                                CountContext.Observe(HandlingFixtures.group, StockFixtures.references),
                                CountFields(quantity = "18", confirmed = true),
                            ),
                    ),
                    CountPresenter(null, null, null, null, rememberCoroutineScope(), OperationIdSource { StockFixtures.operation }),
                    { error("Stale count must not submit") },
                )
            }
            onNodeWithText("Fictional apples · Original crate · Receiving").performScrollTo().assertIsDisplayed()
            onNodeWithTag("count-confirm").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("count-submit").performScrollTo().assertIsNotEnabled()
        }

    @Test
    fun uncertainOriginalExposesLookupWithoutAnotherCommitAction() =
        runComposeUiTest {
            val input =
                HubCountObserveInput(
                    2UL,
                    5UL,
                    HandlingFixtures.group.group.id,
                    1UL,
                    18U,
                    HubOccurrence(HubOccurrenceKind.Known, 100L),
                    "Counted original packs",
                )
            setHarvestCircleContent {
                CountWorkspace(
                    CountViewState(
                        available = true,
                        open = true,
                        submission =
                            CountSubmission.Uncertain(
                                CountAttempt(StockFixtures.operation, CountRequest.Observe(input)),
                                "Original result unconfirmed",
                            ),
                    ),
                    CountPresenter(null, null, null, null, rememberCoroutineScope(), OperationIdSource { StockFixtures.operation }),
                    { error("Uncertain operation must not submit") },
                )
            }
            onNodeWithTag("count-uncertain").assertIsDisplayed()
            onNodeWithTag("count-check-original").performScrollTo().assertIsDisplayed()
            onNodeWithTag("count-submit").assertDoesNotExist()
            onNodeWithTag("count-confirm").assertDoesNotExist()
        }
}
