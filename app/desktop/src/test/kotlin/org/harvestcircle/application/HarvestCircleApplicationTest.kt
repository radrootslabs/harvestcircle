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
import org.harvestcircle.ffi.AppLifecycleDto
import org.harvestcircle.ffi.AppSnapshotDto
import org.harvestcircle.ffi.SessionStateDto
import kotlin.test.Test
import kotlin.test.assertEquals

class HarvestCircleApplicationTest {
    @OptIn(ExperimentalTestApi::class)
    @Test
    fun applicationCreatesOneStoreAcrossRecompositionAndClosesItOnDisposal() =
        runComposeUiTest {
            var applicationVisible by mutableStateOf(true)
            var factoryCalls = 0
            var gateway: ApplicationGateway? = null

            setContent {
                if (applicationVisible) {
                    HarvestCircleApplication { scope ->
                        factoryCalls += 1
                        val createdGateway = ApplicationGateway()
                        gateway = createdGateway
                        HarvestCircleAppStore(createdGateway, scope)
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
            waitForIdle()

            assertEquals(1, factoryCalls)
            assertEquals(true, gateway?.closed)
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

private class ApplicationGateway : HarvestCircleCoreGateway {
    var closed = false

    override fun snapshot() = applicationSnapshot(0UL)

    override suspend fun subscribeChanges(onChange: (HarvestCircleChange) -> Unit) = AutoCloseable {}

    override suspend fun execute(command: HarvestCircleCommand): HarvestCircleCommandResult = error("unused")

    override suspend fun bootstrap() = applicationSnapshot(1UL)

    override suspend fun beginGeneratedAccount(): GeneratedRecoveryTicket = error("unused")

    override suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket = error("unused")

    override suspend fun confirmAccountRemoval(ticket: RemovalTicket) = error("unused")

    override fun shutdown(): HarvestCircleShutdownReceipt {
        closed = true
        return HarvestCircleShutdownReceipt(1UL, closed = true)
    }

    override fun close() {
        shutdown()
    }
}

private fun applicationSnapshot(revision: ULong) =
    AppSnapshotDto(
        revision = revision,
        lifecycle = AppLifecycleDto.READY,
        lifecycleError = null,
        configuredRelays = emptyList(),
        accounts = emptyList(),
        selectedPublicKeyHex = null,
        session = SessionStateDto.SIGNED_OUT,
        sessionSubjectPublicKeyHex = null,
        sessionError = null,
        activeAccount = null,
        recoverableProblem = null,
    )
