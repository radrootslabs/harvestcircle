package org.harvestcircle.ui.shell

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.application.ApplicationErrorCategory
import org.harvestcircle.application.ApplicationErrorCode
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.application.ApplicationProblem
import org.harvestcircle.application.ApplicationSnapshot
import org.harvestcircle.application.HarvestCirclePresenterState
import org.harvestcircle.application.RecoveryAction
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class LifecycleCanvasesTest {
    @Test
    fun blockedLifecycleUsesTheThemeAndTypedRecoveryActions() =
        runComposeUiTest {
            var retries = 0
            var dismissals = 0
            setHarvestCircleContent {
                HarvestCircleTheme(org.harvestcircle.appearance.AppearanceState()) {
                    ShellLifecycleCanvas(
                        presenterState(),
                        HarvestCircleUiActions(
                            retryLastCommand = { retries += 1 },
                            dismissProblem = { dismissals += 1 },
                        ),
                    )
                }
            }

            onNodeWithTag("lifecycle-blocked").assertExists()
            onNodeWithText("Retry").performClick()
            onNodeWithText("Dismiss").performClick()
            assertEquals(1, retries)
            assertEquals(1, dismissals)
        }

    @Test
    fun standaloneFailuresUseTheActiveShellControls() =
        runComposeUiTest {
            var forced = 0
            setHarvestCircleContent { ShutdownFailureScreen("Shutdown timed out.") { forced += 1 } }
            onNodeWithTag("shutdown-problem").assertExists()
            onNodeWithTag("force-exit").performClick()
            assertEquals(1, forced)
        }
}

private fun presenterState(): HarvestCirclePresenterState {
    val problem =
        ApplicationProblem(
            ApplicationErrorCode.StorageUnavailable,
            ApplicationErrorCategory.Storage,
            retryable = true,
            RecoveryAction.RepairStorage,
            operationId = null,
            safeMessage = "The local store needs attention.",
        )
    return HarvestCirclePresenterState(
        snapshot =
            ApplicationSnapshot(
                SnapshotRevision(1UL),
                ApplicationLifecycle.Blocked,
                lifecycleProblem = problem,
                configuredRelays = emptyList(),
                identities = emptyList(),
                selectedIdentityId = null,
                session = SessionLifecycle.SignedOut,
                sessionSubjectIdentityId = null,
                sessionProblem = null,
                activeIdentity = null,
                recoverableProblem = null,
            ),
        lastProblem = problem,
        problem = problem.safeMessage,
    )
}
