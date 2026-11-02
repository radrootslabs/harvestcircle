package org.harvestcircle.integration

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import kotlinx.coroutines.runBlocking
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.application.CountPresenter
import org.harvestcircle.application.CountRequest
import org.harvestcircle.application.CountResult
import org.harvestcircle.application.CountSubmission
import org.harvestcircle.application.HarvestCircleCountRuntime
import org.harvestcircle.application.HubAcceptanceStatus
import org.harvestcircle.application.HubCountBlockReason
import org.harvestcircle.application.HubCountDifference
import org.harvestcircle.application.HubCountObservation
import org.harvestcircle.application.HubCountObserveInput
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubPopulationOrigin
import org.harvestcircle.application.HubSourceCertainty
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.RequestContext
import org.harvestcircle.ui.shell.CountWorkspace
import org.harvestcircle.ui.shell.HarvestCircleTheme
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class CountOpeningAndFaultAcceptanceTest {
    @Test
    fun openingFormReplaysOnceAndSurplusRequiresSeparatePhysicalIdentification() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-opening-form-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            lateinit var presenter: CountPresenter
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                setContent { NativeCountForm(runtime, runtime, { it.beginOpening() }) { presenter = it } }
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("count-variant").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("count-variant").performScrollTo().performClick()
                onNodeWithTag("count-variant-${hub.receipt.trace.variant.id}").performScrollTo().performClick()
                onNodeWithTag("count-location").performScrollTo().performClick()
                onNodeWithTag("count-location-${hub.a3}").performScrollTo().performClick()
                onNodeWithTag("count-quantity").performScrollTo().performTextInput("8")
                onNodeWithTag("count-label").performScrollTo().performTextInput("Opening eight")
                onNodeWithTag("count-identification").performScrollTo().performTextInput("Eight original packs in the marked opening stack")
                onNodeWithTag("count-observation").performScrollTo().performTextInput("Earlier intake and source remain unknown")
                onNodeWithTag("count-submit").assertIsNotEnabled()
                onNodeWithTag("count-confirm").performScrollTo().performClick()
                onNodeWithTag("count-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CountSubmission.Recorded }
                val openingResult = assertIs<CountSubmission.Recorded>(presenter.state.value.submission)
                val opened = assertIs<CountResult.Opened>(openingResult.result).receipt
                assertEquals(HubPopulationOrigin.CountedOpening(null), opened.original.population.origin)
                assertEquals(HubSourceCertainty.Unknown, opened.original.population.source.certainty)
                assertEquals(HubAcceptanceStatus.ReviewRequired, opened.original.acceptance)
                assertTrue(
                    opened.original.group.work
                        .isEmpty(),
                )
                val beforeReplay = runBlocking { runtime.stock(HubStockQuery()) }
                val openingInput = assertIs<CountRequest.Opening>(openingResult.attempt.request).input
                val replay =
                    runBlocking {
                        runtime.recordOpening(
                            RequestContext(opened.operationId, runtime.currentSnapshot().revision, 10_000UL),
                            openingInput,
                        )
                    }
                assertEquals(opened, replay)
                assertEquals(beforeReplay, runBlocking { runtime.stock(HubStockQuery()) })
                onNodeWithTag("count-done").performScrollTo().performClick()
                runOnIdle { presenter.beginObservation(opened.original.group.id) }
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null }
                onNodeWithTag("count-quantity").performScrollTo().performTextInput("10")
                onNodeWithTag("count-observation").performScrollTo().performTextInput("Two extra original packs observed separately")
                onNodeWithTag("count-confirm").performScrollTo().performClick()
                onNodeWithTag("count-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CountSubmission.Recorded }
                val observed =
                    assertIs<CountResult.Observed>(
                        assertIs<CountSubmission.Recorded>(presenter.state.value.submission).result,
                    ).observation
                onNodeWithTag("count-review-post").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null && presenter.state.value.submission == null }
                onNodeWithTag("count-observation").performScrollTo().performTextInput("Reviewed the independently identified surplus")
                onNodeWithTag("count-confirm").performScrollTo().performClick()
                onNodeWithTag("count-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CountSubmission.Rejected }
                assertEquals(8U, runBlocking { runtime.group(opened.original.group.id) }.group.wholePacks)
                onNodeWithText("Review retained input").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.review != null && !presenter.state.value.loading }
                onNodeWithTag("count-accept-review").performScrollTo().performClick()
                onNodeWithTag("count-label").performScrollTo().performTextInput("Separate extra two")
                onNodeWithTag(
                    "count-identification",
                ).performScrollTo().performTextInput("Two red-tagged packs distinct from the opening eight")
                onNodeWithTag("count-submit").assertIsNotEnabled()
                onNodeWithTag("count-confirm").performScrollTo().performClick()
                onNodeWithTag("count-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CountSubmission.Recorded }
                val posted =
                    assertIs<CountResult.Posted>(
                        assertIs<CountSubmission.Recorded>(presenter.state.value.submission).result,
                    ).receipt
                val surplus = assertNotNull(posted.surplus)
                assertIs<HubCountDifference.Surplus>(posted.difference)
                assertEquals(8U, posted.onHandAfter)
                assertEquals(2U, surplus.population.originalWholePacks)
                assertEquals(HubSourceCertainty.Unknown, surplus.population.source.certainty)
                assertEquals(HubAcceptanceStatus.ReviewRequired, surplus.acceptance)
                assertEquals(HubPopulationOrigin.CountedSurplus(observed.id, opened.original.population.id), surplus.population.origin)
                assertTrue(surplus.group.work.isEmpty())
                assertEquals("Separate extra two", surplus.group.identification?.label)
                val rows = runBlocking { runtime.stock(HubStockQuery()) }.rows
                assertEquals(8U, rows.single { it.trace.id == opened.original.population.id }.position.wholePacks)
                assertEquals(2U, rows.single { it.trace.id == surplus.population.id }.position.wholePacks)
                assertNull(rows.single { it.trace.id == surplus.population.id }.putAway)
                assertEquals(opened, runBlocking { runtime.openingOperation(opened.operationId) })
                assertEquals(hub.receipt, runBlocking { runtime.receiptOperation(hub.receipt.operationId) })
            } finally {
                runOnIdle { presenter.close() }
                runBlocking { runtime.close() }
                root.toFile().deleteRecursively()
            }
        }

    @Test
    fun lostObservationReplyStaysUncertainAndMovedScopeCannotBePosted() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-count-fault-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            lateinit var presenter: CountPresenter
            var writes = 0
            var hideFirstOriginal = true
            val counts =
                object : HarvestCircleCountRuntime by runtime {
                    override suspend fun observeCount(
                        context: RequestContext,
                        input: HubCountObserveInput,
                    ): HubCountObservation {
                        writes += 1
                        runtime.observeCount(context, input)
                        error("Injected loss after actual native commit")
                    }

                    override suspend fun countObservationOperation(operation: OperationId): HubCountObservation? {
                        val original = runtime.countObservationOperation(operation)
                        if (hideFirstOriginal) {
                            hideFirstOriginal = false
                            return null
                        }
                        return original
                    }
                }
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                setContent {
                    NativeCountForm(
                        runtime,
                        counts,
                        { it.beginObservation(hub.receipt.initialPosition.groupId) },
                    ) { presenter = it }
                }
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null }
                onNodeWithTag("count-quantity").performScrollTo().performTextInput("18")
                onNodeWithTag("count-observation").performScrollTo().performTextInput("Counted eighteen before the later movement")
                onNodeWithTag("count-confirm").performScrollTo().performClick()
                onNodeWithTag("count-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CountSubmission.Uncertain }
                val uncertain = assertIs<CountSubmission.Uncertain>(presenter.state.value.submission)
                onNodeWithTag("count-submit").assertDoesNotExist()
                onNodeWithTag("count-check-original").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    !presenter.state.value.loading &&
                        presenter.state.value.submission is CountSubmission.Uncertain
                }
                assertEquals(uncertain.attempt, assertIs<CountSubmission.Uncertain>(presenter.state.value.submission).attempt)
                assertEquals(1, writes)
                onNodeWithTag("count-submit").assertDoesNotExist()
                onNodeWithTag("count-check-original").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CountSubmission.Recorded }
                assertEquals(
                    uncertain.attempt.operation,
                    assertIs<CountSubmission.Recorded>(presenter.state.value.submission).result.operation,
                )
                val saved = runBlocking { runtime.stock(HubStockQuery()) }.rows.single()
                assertEquals(20U, saved.position.wholePacks)
                assertEquals(20U, saved.putAway?.remaining)
                runBlocking {
                    val group = runtime.group(hub.receipt.initialPosition.groupId)
                    runtime.applyHandling(hub.context(), hub.move(group, hub.a3, 20U))
                }
                val beforeReview = runBlocking { runtime.stock(HubStockQuery()) }
                onNodeWithTag("count-review-post").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null && presenter.state.value.submission == null }
                onNodeWithText("Physical scope changed", substring = true).assertExists()
                onNodeWithTag("count-confirm").assertIsNotEnabled()
                onNodeWithTag("count-submit").assertIsNotEnabled()
                assertEquals(beforeReview, runBlocking { runtime.stock(HubStockQuery()) })
                assertEquals(1, writes)
                assertEquals(hub.receipt, runBlocking { runtime.receiptOperation(hub.receipt.operationId) })
            } finally {
                runOnIdle { presenter.close() }
                runBlocking { runtime.close() }
                root.toFile().deleteRecursively()
            }
        }

    @Test
    fun unknownCountCutoffIsSavedAsEvidenceAndBlocksReconciliation() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-count-unknown-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            lateinit var presenter: CountPresenter
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                setContent {
                    NativeCountForm(runtime, runtime, { it.beginObservation(hub.receipt.initialPosition.groupId) }) {
                        presenter =
                            it
                    }
                }
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null }
                onNodeWithTag("count-quantity").performScrollTo().performTextInput("18")
                onNodeWithTag("count-cutoff-kind").performScrollTo().performClick()
                onNodeWithText("Unknown").performClick()
                onNodeWithTag("count-observation").performScrollTo().performTextInput("Count time cannot be established")
                onNodeWithTag("count-confirm").performScrollTo().performClick()
                onNodeWithTag("count-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is CountSubmission.Recorded }
                val observed =
                    assertIs<CountResult.Observed>(
                        assertIs<CountSubmission.Recorded>(presenter.state.value.submission).result,
                    ).observation
                assertEquals(HubOccurrenceKind.Unknown, observed.cutoff.kind)
                assertNull(observed.cutoff.startUnixSeconds)
                val before = runBlocking { runtime.stock(HubStockQuery()) }
                onNodeWithTag("count-review-post").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null && presenter.state.value.submission == null }
                assertTrue(HubCountBlockReason.CutoffUncertain in runBlocking { runtime.countReview(observed.id) }.blockers)
                onNodeWithText("Count cutoff is uncertain", substring = true).assertExists()
                onNodeWithTag("count-confirm").assertIsNotEnabled()
                onNodeWithTag("count-submit").assertIsNotEnabled()
                assertEquals(before, runBlocking { runtime.stock(HubStockQuery()) })
            } finally {
                runOnIdle { presenter.close() }
                runBlocking { runtime.close() }
                root.toFile().deleteRecursively()
            }
        }
}

@Composable
private fun NativeCountForm(
    runtime: TestBridgeHarvestCircleRuntime,
    counts: HarvestCircleCountRuntime,
    start: (CountPresenter) -> Unit,
    capture: (CountPresenter) -> Unit,
) {
    val scope = rememberCoroutineScope()
    val presenter =
        remember {
            CountPresenter(
                counts,
                runtime,
                runtime,
                runtime,
                scope,
                OperationIdSource { OperationId.from(org.harvestcircle.ffi.generateOperationIdV7()) },
                { "2023-11-14T22:13:20Z" },
            )
        }
    capture(presenter)
    DisposableEffect(presenter) { onDispose { presenter.close() } }
    LaunchedEffect(presenter) {
        presenter.attach("isolated-opening-and-fault-form")
        start(presenter)
    }
    val state by presenter.state.collectAsState()
    HarvestCircleTheme(AppearanceState()) { CountWorkspace(state, presenter) { presenter.submit(runtime.currentSnapshot().revision) } }
}
