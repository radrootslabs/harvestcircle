package org.radroots.studio.application

import androidx.compose.foundation.clickable
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test
import kotlin.test.assertEquals
import org.radroots.studio.ffi.AppLifecycleDto
import org.radroots.studio.ffi.AppSnapshotDto
import org.radroots.studio.ffi.SessionStateDto

class RadrootsApplicationTest {
    @OptIn(ExperimentalTestApi::class)
    @Test
    fun applicationCreatesOneStoreAcrossRecompositionAndClosesItOnDisposal() =
        runComposeUiTest {
            var applicationVisible by mutableStateOf(true)
            var factoryCalls = 0
            var gateway: ApplicationGateway? = null

            setContent {
                if (applicationVisible) {
                    RadrootsApplication { scope ->
                        factoryCalls += 1
                        val createdGateway = ApplicationGateway()
                        gateway = createdGateway
                        StudioAppStore(createdGateway, scope)
                    }
                }
                BasicText(
                    text = "Toggle",
                    modifier = Modifier
                        .testTag("toggle-application")
                        .clickable { applicationVisible = !applicationVisible },
                )
            }

            onNodeWithText("radroots").assertIsDisplayed()
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
                RadrootsApplication {
                    error("sensitive internal startup detail")
                }
            }

            onNodeWithTag("startup-failure").assertIsDisplayed()
            onNodeWithText("The application could not start.").assertIsDisplayed()
            onAllNodesWithText("sensitive internal startup detail").assertCountEquals(0)
        }
}

private class ApplicationGateway : StudioCoreGateway {
    var closed = false

    override fun snapshot() = applicationSnapshot(0UL)
    override suspend fun subscribe(onSnapshot: (org.radroots.studio.ffi.AppSnapshotDto) -> Unit) =
        AutoCloseable {}
    override suspend fun bootstrap() = applicationSnapshot(1UL)
    override suspend fun generateAccount(): org.radroots.studio.ffi.GeneratedAccountDto =
        error("unused")
    override suspend fun importSecretKey(secretKey: ByteArray) = error("unused")
    override suspend fun selectAccount(publicKeyHex: String) = error("unused")
    override suspend fun activateAccount(publicKeyHex: String) = error("unused")
    override suspend fun signOut() = error("unused")
    override suspend fun refreshActiveProfile() = error("unused")
    override suspend fun requestAccountRemoval(publicKeyHex: String): RemovalTicket = error("unused")
    override suspend fun confirmAccountRemoval(ticket: RemovalTicket) = error("unused")

    override fun close() {
        closed = true
    }
}

private fun applicationSnapshot(revision: ULong) = AppSnapshotDto(
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
