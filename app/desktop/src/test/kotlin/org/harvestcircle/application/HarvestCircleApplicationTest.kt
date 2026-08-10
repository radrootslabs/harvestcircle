package org.harvestcircle.application

import androidx.compose.foundation.clickable
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.emptyFlow
import kotlin.test.Test
import kotlin.test.assertEquals

class HarvestCircleApplicationTest {
    @OptIn(ExperimentalTestApi::class)
    @Test
    fun applicationCreatesOnePresenterAcrossRecompositionAndClosesItOnDisposal() =
        runComposeUiTest {
            var applicationVisible by mutableStateOf(true)
            var factoryCalls = 0
            var runtime: ApplicationRuntime? = null

            setContent {
                if (applicationVisible) {
                    HarvestCircleApplication { scope ->
                        factoryCalls += 1
                        val createdRuntime = ApplicationRuntime()
                        runtime = createdRuntime
                        HarvestCirclePresenter(
                            runtime = createdRuntime,
                            scope = scope,
                            clock = ApplicationClock { UnixSeconds(0) },
                            operationIds = OperationIdSource { OperationId.from("application-test") },
                        )
                    }
                }
                BasicText(
                    text = "Toggle",
                    modifier =
                        Modifier
                            .testTag("toggle-application")
                            .clickable { applicationVisible = !applicationVisible },
                )
            }

            onNodeWithText("HarvestCircle").assertIsDisplayed()
            onNodeWithTag("toggle-application").performClick()
            waitUntil { runtime?.closed == true }

            assertEquals(1, factoryCalls)
            assertEquals(true, runtime?.closed)
        }

    @OptIn(ExperimentalTestApi::class)
    @Test
    fun applicationRendersSafeStartupFailureWithoutLeakingInternalMessage() =
        runComposeUiTest {
            setContent {
                HarvestCircleApplication {
                    error("sensitive internal startup detail")
                }
            }

            onNodeWithTag("startup-failure").assertIsDisplayed()
            onNodeWithText("The application could not start.").assertIsDisplayed()
            onAllNodesWithText("sensitive internal startup detail").assertCountEquals(0)
        }
}

private class ApplicationRuntime : HarvestCircleRuntime {
    var closed = false

    override suspend fun bootstrap(): ApplicationSnapshot = applicationSnapshot(SnapshotRevision(1UL))

    override fun currentSnapshot(): ApplicationSnapshot = applicationSnapshot(SnapshotRevision(0UL))

    override fun changes(): Flow<ApplicationChange> = emptyFlow()

    override suspend fun execute(command: ApplicationCommand): ApplicationCommandResult = error("unused")

    override suspend fun prepareLocalIdentity(): GeneratedIdentityRecovery = error("unused")

    override suspend fun requestIdentityRemoval(identityId: IdentityId): IdentityRemovalRequest = error("unused")

    override suspend fun cancelIdentityRemoval(requestId: RemovalRequestId): Boolean = false

    override suspend fun shutdown(): ShutdownReceipt {
        closed = true
        return ShutdownReceipt(SnapshotRevision(1UL), closed = true)
    }
}

private fun applicationSnapshot(revision: SnapshotRevision) =
    ApplicationSnapshot(
        revision = revision,
        lifecycle = ApplicationLifecycle.Ready,
        lifecycleProblem = null,
        configuredRelays = emptyList(),
        identities = emptyList(),
        selectedIdentityId = null,
        session = SessionLifecycle.SignedOut,
        sessionSubjectIdentityId = null,
        sessionProblem = null,
        activeIdentity = null,
        recoverableProblem = null,
    )
