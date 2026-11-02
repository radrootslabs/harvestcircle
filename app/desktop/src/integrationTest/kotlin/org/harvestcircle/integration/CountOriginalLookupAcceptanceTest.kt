package org.harvestcircle.integration

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
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
import org.harvestcircle.application.ApplicationFailure
import org.harvestcircle.application.CountLookupKind
import org.harvestcircle.application.HarvestCircleApplicationWithDependencies
import org.harvestcircle.application.HarvestCirclePresenter
import org.harvestcircle.application.HubCountObserveInput
import org.harvestcircle.application.HubCountPostInput
import org.harvestcircle.application.HubHandlingQuery
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubOpeningInput
import org.harvestcircle.application.HubPhysicalIdentification
import org.harvestcircle.application.HubSourceInput
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.SecretClipboardController
import org.harvestcircle.application.TextClipboard
import org.harvestcircle.application.UnixSeconds
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeEnvironment
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeExclusion
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

@OptIn(ExperimentalTestApi::class)
class CountOriginalLookupAcceptanceTest {
    @Test
    fun reopenedStockLooksUpAllOriginalKindsWithoutRepeatingEffects() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-original-count-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            var show by mutableStateOf(true)
            var closing by mutableStateOf(false)
            var exits = 0
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                val originals =
                    runBlocking {
                        val stock = runtime.stock(HubStockQuery())
                        val opened =
                            runtime.recordOpening(
                                hub.context(),
                                HubOpeningInput(
                                    stock.stockRevision,
                                    stock.catalogRevision,
                                    hub.receipt.trace.variant.id,
                                    hub.a3,
                                    8U,
                                    HubPhysicalIdentification("Opening eight", "Separate counted stack"),
                                    HubSourceInput(),
                                    null,
                                    HubOccurrence(HubOccurrenceKind.Known, 1_700_000_000L),
                                    "Historical intake unknown",
                                ),
                            )
                        val group = runtime.group(hub.receipt.initialPosition.groupId)
                        val observed =
                            runtime.observeCount(
                                hub.context(),
                                HubCountObserveInput(
                                    group.stockRevision,
                                    group.catalogRevision,
                                    group.group.id,
                                    group.group.revision,
                                    18U,
                                    HubOccurrence(HubOccurrenceKind.Known, 1_700_000_000L),
                                    "Counted original eighteen",
                                ),
                            )
                        val review = runtime.countReview(observed.id)
                        val posted =
                            runtime.postCount(
                                hub.context(),
                                HubCountPostInput(
                                    review.stockRevision,
                                    review.catalogRevision,
                                    observed.id,
                                    true,
                                    null,
                                    "Reviewed native shortage",
                                ),
                            )
                        runtime.restart()
                        assertFailsWith<ApplicationFailure> { runtime.openingOperation(opened.operationId) }
                        assertFailsWith<ApplicationFailure> { runtime.countObservationOperation(observed.operationId) }
                        assertFailsWith<ApplicationFailure> { runtime.countPostOperation(posted.operationId) }
                        val foreign = runtime.prepareLocalIdentity()
                        try {
                            runtime.execute(ApplicationCommand.AcknowledgeGeneratedIdentity(foreign.requestId, hub.context()))
                            runtime.execute(ApplicationCommand.ActivateIdentity(foreign.identity.id))
                            assertFailsWith<ApplicationFailure> { runtime.openingOperation(opened.operationId) }
                            assertFailsWith<ApplicationFailure> { runtime.countObservationOperation(observed.operationId) }
                            assertFailsWith<ApplicationFailure> { runtime.countPostOperation(posted.operationId) }
                        } finally {
                            foreign.backup.clear()
                        }
                        runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                        Triple(opened, observed, posted)
                    }
                val before = runBlocking { runtime.stock(HubStockQuery()) }
                val workBefore = runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }
                setContent {
                    HarvestCircleWindowChromeEnvironment(HarvestCircleWindowChromeExclusion.None) {
                        if (show) {
                            HarvestCircleApplicationWithDependencies(
                                closeRequested = closing,
                                onExitApproved = { exits += 1 },
                                clipboardFactory = { scope -> SecretClipboardController(scope, LookupClipboard()) },
                            ) { scope ->
                                HarvestCirclePresenter(
                                    runtime,
                                    scope,
                                    ApplicationClock { UnixSeconds(1_700_000_000) },
                                    OperationIdSource { error("Read-only lookup cannot allocate identity operations") },
                                )
                            }
                        }
                    }
                }
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("sidebar-HubStock").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("sidebar-HubStock").performClick()
                for ((kind, operation) in listOf(
                    CountLookupKind.Opening to originals.first.operationId,
                    CountLookupKind.Observation to originals.second.operationId,
                    CountLookupKind.Reconciliation to originals.third.operationId,
                )) {
                    waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("count-lookup").assertIsEnabled() }.isSuccess }
                    onNodeWithTag("count-lookup").performScrollTo().performClick()
                    waitUntil(
                        timeoutMillis = 10_000,
                    ) { onNodeWithTag("count-original-lookup").runCatching { fetchSemanticsNode() }.isSuccess }
                    if (kind != CountLookupKind.Opening) {
                        onNodeWithTag("count-lookup-kind").performScrollTo().performClick()
                        onNodeWithText(kind.name).performClick()
                    }
                    onNodeWithTag("count-lookup-operation").performScrollTo().performTextInput(operation.value)
                    onNodeWithTag("count-lookup-read").performScrollTo().performClick()
                    waitUntil(
                        timeoutMillis = 10_000,
                    ) { onNodeWithTag("count-lookup-result").runCatching { fetchSemanticsNode() }.isSuccess }
                    onNodeWithText("Original operation: ${operation.value}").assertExists()
                    if (kind != CountLookupKind.Opening) {
                        onNodeWithText("Original physical fact: Fictional apples · Original crate · Receiving").assertExists()
                        onNodeWithText("Original observation: Counted original eighteen").assertExists()
                        onNodeWithText("Observed by ${hub.owner.value} · Recorded 2023-11-14T22:13:20Z").assertExists()
                        onNodeWithText("Original cutoff: Known · 2023-11-14T22:13:20Z · No interval end").assertExists()
                    }
                    if (kind == CountLookupKind.Reconciliation) {
                        onNodeWithText("Original reconciliation reason: Reviewed native shortage").assertExists()
                        onNodeWithText("Reconciled by ${hub.owner.value} · Recorded 2023-11-14T22:13:20Z").assertExists()
                    }

                    onNodeWithTag("count-submit").assertDoesNotExist()
                    assertEquals(before, runBlocking { runtime.stock(HubStockQuery()) })
                    assertEquals(workBefore, runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) })
                    onNodeWithTag("count-lookup-close").performScrollTo().performClick()
                }
                onNodeWithTag("count-lookup").performScrollTo().performClick()
                onNodeWithTag("count-lookup-operation").performScrollTo().performTextInput(originals.second.operationId.value)
                onNodeWithTag("count-lookup-read").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("count-message").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("count-lookup-result").assertDoesNotExist()
                onNodeWithTag("count-submit").assertDoesNotExist()
                assertEquals(before, runBlocking { runtime.stock(HubStockQuery()) })
                onNodeWithTag("count-lookup-close").performScrollTo().performClick()
                onNodeWithTag("count-lookup").performScrollTo().performClick()
                onNodeWithTag("count-lookup-operation").performScrollTo().performTextInput("01900000-0000-7000-8000-000000000999")
                onNodeWithTag("count-lookup-read").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    onNodeWithText("No original of this kind is visible.", substring = true).runCatching { fetchSemanticsNode() }.isSuccess
                }
                onNodeWithTag("count-lookup-result").assertDoesNotExist()
                onNodeWithTag("count-submit").assertDoesNotExist()
                assertEquals(before, runBlocking { runtime.stock(HubStockQuery()) })
                assertEquals(workBefore, runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) })
                closing = true
                waitUntil(timeoutMillis = 10_000) { exits == 1 }
                show = false
                waitForIdle()
            } finally {
                show = false
                runBlocking { runtime.close() }
                root.toFile().deleteRecursively()
            }
        }
}

private class LookupClipboard : TextClipboard {
    override fun readText(): String? = null

    override fun writeText(value: String) = Unit
}
