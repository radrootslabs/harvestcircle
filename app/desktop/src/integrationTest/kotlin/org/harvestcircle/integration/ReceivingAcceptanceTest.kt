package org.harvestcircle.integration

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationClock
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.HarvestCircleApplicationWithDependencies
import org.harvestcircle.application.HarvestCirclePresenter
import org.harvestcircle.application.HubLocationPurpose
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.RequestContext
import org.harvestcircle.application.SecretClipboardController
import org.harvestcircle.application.TextClipboard
import org.harvestcircle.application.UnixSeconds
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeEnvironment
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeExclusion
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class ReceivingAcceptanceTest {
    @Test
    fun receivingUsesNativeStorageAndSurvivesRestart() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-receiving-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            var showApplication by mutableStateOf(true)
            var closeRequested by mutableStateOf(false)
            var approvedExits = 0
            val ids = OperationIdSource { OperationId.from(org.harvestcircle.ffi.generateOperationIdV7()) }

            fun context() = RequestContext(ids.next(), runtime.currentSnapshot().revision, 10_000UL)
            try {
                val owner =
                    runBlocking {
                        runtime.bootstrap()
                        val generated = runtime.prepareLocalIdentity()
                        runtime.execute(ApplicationCommand.AcknowledgeGeneratedIdentity(generated.requestId, context()))
                        generated.backup.clear()
                        runtime.execute(ApplicationCommand.ActivateIdentity(generated.identity.id))
                        runtime.initializeHub(context())
                        generated.identity.id
                    }

                fun add(value: HubReferenceValue): String =
                    runBlocking {
                        val catalog = runtime.references(HubReferenceQuery())
                        runtime.editReference(context(), HubReferenceEdit(catalog.catalogRevision, value = value)).current.id
                    }
                val product = add(HubReferenceValue(HubReferenceKind.Product, "Fictional apples"))
                val pack = add(HubReferenceValue(HubReferenceKind.Pack, "Original crate", productId = product))
                val receiving =
                    add(HubReferenceValue(HubReferenceKind.Location, "Receiving", locationPurpose = HubLocationPurpose.Receiving))
                val planned = add(HubReferenceValue(HubReferenceKind.Location, "A3", locationPurpose = HubLocationPurpose.Storage))
                setContent {
                    HarvestCircleWindowChromeEnvironment(HarvestCircleWindowChromeExclusion.None) {
                        if (showApplication) {
                            HarvestCircleApplicationWithDependencies(
                                closeRequested = closeRequested,
                                onExitApproved = { approvedExits += 1 },
                                clipboardFactory = { scope ->
                                    SecretClipboardController(scope, ReceivingClipboard(), clearDelayMillis = 60_000)
                                },
                            ) { scope ->
                                HarvestCirclePresenter(runtime, scope, ApplicationClock { UnixSeconds(1_700_000_000) }, ids)
                            }
                        }
                    }
                }
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("stock-receive").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("sidebar-HubStock").performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("stock-receive").assertIsEnabled() }.isSuccess }
                onNodeWithTag("stock-receive").performClick()
                onNodeWithTag("receipt-variant").performScrollTo().performClick()
                onNodeWithTag("receipt-variant-$pack").performScrollTo().performClick()
                onNodeWithTag("receipt-quantity").performScrollTo().performTextInput("20")
                onNodeWithTag("receipt-receiving").performScrollTo().performClick()
                onNodeWithTag("receipt-receiving-$receiving").performScrollTo().performClick()
                onNodeWithTag("receipt-planned").performScrollTo().performClick()
                onNodeWithTag("receipt-planned-$planned").performScrollTo().performClick()
                onNodeWithTag("receipt-observation").performScrollTo().performTextInput("Manually counted twenty original crates")
                onNodeWithTag("receipt-save").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("receipt-recorded").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("receipt-recorded").performScrollTo().assertIsDisplayed()
                val first = runBlocking { runtime.stock(HubStockQuery()) }
                assertEquals(1U, first.total)
                assertEquals(
                    20U,
                    first.rows
                        .single()
                        .position.wholePacks,
                )
                assertEquals(
                    receiving,
                    first.rows
                        .single()
                        .position.locationId,
                )
                assertEquals(
                    planned,
                    first.rows
                        .single()
                        .putAway.target
                        ?.id,
                )
                assertEquals(
                    0U,
                    first.rows
                        .single()
                        .putAway.completed,
                )
                assertEquals(
                    20U,
                    first.rows
                        .single()
                        .putAway.remaining,
                )
                assertTrue(
                    first.rows
                        .single()
                        .trace.sourceNeedsReview,
                )
                assertEquals(0U, runBlocking { runtime.stock(HubStockQuery(locationId = planned)) }.total)
                onNodeWithTag("receipt-done").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithText("At: Receiving").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("sidebar-HubWork").performClick()
                onNodeWithTag("stock-work-remaining").performScrollTo().assertIsDisplayed()
                closeRequested = true
                waitUntil(timeoutMillis = 10_000) { approvedExits == 1 }
                showApplication = false
                waitForIdle()
                // Reopen the same real database with the test-owned credential custody retained.
                runBlocking {
                    runtime.restart()
                    runtime.execute(ApplicationCommand.ActivateIdentity(owner))
                    val reopened = runtime.stock(HubStockQuery())
                    assertEquals(first, reopened)
                    assertNotNull(runtime.hubContext())
                }
            } finally {
                showApplication = false
                runtime.close()
                Files.walk(root).use { paths ->
                    paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists)
                }
            }
        }
}

private class ReceivingClipboard : TextClipboard {
    private var value: String? = null

    override fun readText(): String? = value

    override fun writeText(value: String) {
        this.value = value.ifEmpty { null }
    }
}
