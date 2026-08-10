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
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.emptyFlow
import kotlin.test.Test
import kotlin.test.assertEquals

class HarvestCircleApplicationTest {
    @OptIn(ExperimentalTestApi::class)
    @Test
    fun applicationCreatesOnePresenterAndAwaitsCloseBeforeApprovingExit() =
        runComposeUiTest {
            var closeRequested by mutableStateOf(false)
            var factoryCalls = 0
            var approvedExits = 0
            var runtime: ApplicationRuntime? = null

            setContent {
                HarvestCircleApplication(
                    closeRequested = closeRequested,
                    onExitApproved = { approvedExits += 1 },
                ) { scope ->
                    factoryCalls += 1
                    val createdRuntime = ApplicationRuntime()
                    runtime = createdRuntime
                    HarvestCirclePresenter(
                        runtime = createdRuntime,
                        scope = scope,
                        clock = ApplicationClock { UnixSeconds(0) },
                        operationIds = OperationIdSource { OperationId.from(TEST_OPERATION_ID) },
                    )
                }
                BasicText(
                    text = "Close",
                    modifier =
                        Modifier
                            .testTag("close-application")
                            .clickable { closeRequested = true },
                )
            }

            onNodeWithText("HarvestCircle").assertIsDisplayed()
            onNodeWithTag("close-application").performClick()
            waitUntil { runtime?.closed == true && approvedExits == 1 }

            assertEquals(1, factoryCalls)
            assertEquals(true, runtime?.closed)
        }

    @OptIn(ExperimentalTestApi::class)
    @Test
    fun failedNativeShutdownRequiresAnExplicitForceExitChoice() =
        runComposeUiTest {
            var approvedExits = 0
            setContent {
                HarvestCircleApplication(
                    closeRequested = true,
                    onExitApproved = { approvedExits += 1 },
                ) { scope ->
                    HarvestCirclePresenter(
                        runtime = ApplicationRuntime(shutdownClosed = false),
                        scope = scope,
                        clock = ApplicationClock { UnixSeconds(0) },
                        operationIds = OperationIdSource { OperationId.from(TEST_OPERATION_ID) },
                    )
                }
            }

            onNodeWithTag("shutdown-failure").assertIsDisplayed()
            assertEquals(0, approvedExits)
            onNodeWithTag("force-exit").performClick()
            assertEquals(1, approvedExits)
        }

    @OptIn(ExperimentalTestApi::class)
    @Test
    fun shutdownTimeoutRequiresAnExplicitForceExitChoice() =
        runComposeUiTest {
            var approvedExits = 0
            setContent {
                HarvestCircleApplication(
                    closeRequested = true,
                    onExitApproved = { approvedExits += 1 },
                    shutdownTimeoutMillis = 1,
                ) { scope ->
                    HarvestCirclePresenter(
                        runtime = ApplicationRuntime(shutdownGate = CompletableDeferred()),
                        scope = scope,
                        clock = ApplicationClock { UnixSeconds(0) },
                        operationIds = OperationIdSource { OperationId.from(TEST_OPERATION_ID) },
                    )
                }
            }

            onNodeWithTag("shutdown-failure").assertIsDisplayed()
            assertEquals(0, approvedExits)
            onNodeWithTag("force-exit").performClick()
            assertEquals(1, approvedExits)
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

private class ApplicationRuntime(
    private val shutdownClosed: Boolean = true,
    private val shutdownGate: CompletableDeferred<Unit>? = null,
) : HarvestCircleRuntime {
    var closed = false

    override suspend fun bootstrap(): ApplicationSnapshot = applicationSnapshot(SnapshotRevision(1UL))

    override fun currentSnapshot(): ApplicationSnapshot = applicationSnapshot(SnapshotRevision(0UL))

    override fun changes(): Flow<ApplicationChange> = emptyFlow()

    override suspend fun execute(command: ApplicationCommand): ApplicationCommandResult = error("unused")

    override suspend fun prepareLocalIdentity(): GeneratedIdentityRecovery = error("unused")

    override suspend fun requestIdentityRemoval(identityId: IdentityId): IdentityRemovalRequest = error("unused")

    override suspend fun cancelIdentityRemoval(requestId: RemovalRequestId): Boolean = false

    override suspend fun shutdown(): ShutdownReceipt {
        shutdownGate?.await()
        closed = true
        return ShutdownReceipt(SnapshotRevision(1UL), closed = shutdownClosed)
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

private const val TEST_OPERATION_ID = "01890f3e-7b1c-7000-8000-000000000009"
