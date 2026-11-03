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
import org.harvestcircle.application.ActualPresenter
import org.harvestcircle.application.ActualPreview
import org.harvestcircle.application.ActualResult
import org.harvestcircle.application.ActualSubmission
import org.harvestcircle.application.HarvestCircleActualRuntime
import org.harvestcircle.application.HubActualDepartureInput
import org.harvestcircle.application.HubActualDeparturePreview
import org.harvestcircle.application.HubActualHoldAssessment
import org.harvestcircle.application.HubActualHoldTiming
import org.harvestcircle.application.HubActualReport
import org.harvestcircle.application.HubActualReportInput
import org.harvestcircle.application.HubActualReviewOriginal
import org.harvestcircle.application.HubActualReviewReceipt
import org.harvestcircle.application.HubHandlingInput
import org.harvestcircle.application.HubHandlingQuery
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubPhysicalIdentification
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubTreatmentScope
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.RequestContext
import org.harvestcircle.ui.shell.ActualWorkspace
import org.harvestcircle.ui.shell.HarvestCircleTheme
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class ActualFormAcceptanceTest {
    @Test
    fun unknownReportFormRetainsNativeCommitAfterLostReplyWithoutAnotherOperation() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-actual-report-form-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            lateinit var presenter: ActualPresenter
            var writes = 0
            try {
                runBlocking { HandlingFixture.open(runtime) }
                val before = runBlocking { runtime.stock(HubStockQuery()) }
                val work = runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }
                val lost =
                    object : HarvestCircleActualRuntime by runtime {
                        override suspend fun reportActual(
                            context: RequestContext,
                            input: HubActualReportInput,
                        ): HubActualReport {
                            writes++
                            runtime.reportActual(context, input)
                            error("Test-owned lost reply after native commit")
                        }
                    }
                val hubId = runBlocking { assertNotNull(runtime.hubContext()).id }
                setContent { NativeActualForm(runtime, lost, hubId, { it.beginReport() }) { presenter = it } }
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null }
                onNodeWithTag(
                    "actual-observation",
                ).performScrollTo().performTextInput("Actual departure reported; physical source and quantity not established")
                onNodeWithTag("actual-submit").performScrollTo().assertIsNotEnabled()
                onNodeWithTag("actual-confirm").performScrollTo().performClick()
                onNodeWithTag("actual-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is ActualSubmission.Uncertain }
                val attempt = assertIs<ActualSubmission.Uncertain>(presenter.state.value.submission).attempt
                runOnIdle {
                    presenter.beginReport()
                    presenter.discard()
                    presenter.submit(runtime.currentSnapshot().revision)
                }
                assertEquals(1, writes)
                assertEquals(attempt, assertIs<ActualSubmission.Uncertain>(presenter.state.value.submission).attempt)
                onNodeWithTag("actual-check-original").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is ActualSubmission.Recorded }
                val report =
                    assertIs<ActualResult.Reported>(
                        assertIs<ActualSubmission.Recorded>(presenter.state.value.submission).result,
                    ).value
                assertNull(report.reported.wholePacks)
                assertTrue(report.reported.sourceClaims.isEmpty())
                assertNull(report.reported.recipientId)
                assertEquals(before.rows, runBlocking { runtime.stock(HubStockQuery()) }.rows)
                assertEquals(work.rows, runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }.rows)
                onNodeWithTag("actual-done").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    presenter.state.value.page
                        ?.rows
                        ?.any { it.report == report } == true
                }
                assertEquals(1, writes)
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }

    @Test
    fun establishedBreachFormShowsNativeComparisonAndKeepsCurrentHold() = physicalReview(false)

    @Test
    fun preHoldDepartureFormKeepsHistoricalTimingSeparateFromCurrentHold() = physicalReview(true)

    @Test
    fun uncertainHistoricalScopeKeepsConditionalBreachUnverified() = physicalReview(false, true)

    private fun physicalReview(
        beforeHold: Boolean,
        separateCohort: Boolean = false,
    ) = runDesktopComposeUiTest(width = 1280, height = 800) {
        val root = Files.createTempDirectory("harvestcircle-actual-review-form-")
        val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
        lateinit var presenter: ActualPresenter
        try {
            val hub = runBlocking { HandlingFixture.open(runtime, HubOccurrence(HubOccurrenceKind.Known, 1_699_999_940L)) }
            val groupId = hub.receipt.initialPosition.groupId
            if (separateCohort) {
                runBlocking {
                    val group = runtime.group(groupId)
                    runtime.applyHandling(
                        hub.context(),
                        HubHandlingInput.Split(
                            group.stockRevision,
                            group.catalogRevision,
                            groupId,
                            group.group.revision,
                            6U,
                            HubPhysicalIdentification("Separate six", "Six distinct fictional crates"),
                            HubOccurrence(HubOccurrenceKind.Known, 1_699_999_950L),
                            "Observed separation before hold",
                        ),
                    )
                }
            }
            val beforePacks = if (separateCohort) 14U else 20U
            val afterPacks = if (separateCohort) 12U else 18U
            val recipient =
                runBlocking {
                    val references = runtime.references(HubReferenceQuery())
                    val party =
                        runtime
                            .editReference(
                                hub.context(),
                                HubReferenceEdit(
                                    references.catalogRevision,
                                    value = HubReferenceValue(HubReferenceKind.Party, "Fictional actual recipient"),
                                ),
                            ).current
                    val group = runtime.group(groupId)
                    runtime.applyHandling(
                        hub.context(),
                        HubHandlingInput.PlaceHold(
                            group.stockRevision,
                            group.catalogRevision,
                            listOf(groupId),
                            HubOccurrence(HubOccurrenceKind.Known, 1_699_999_970L),
                            "Current hold remains independent",
                        ),
                    )
                    party
                }
            val held = runBlocking { runtime.treatment(HubTreatmentScope(hub.receipt.trace.id)) }
            val hubId = runBlocking { assertNotNull(runtime.hubContext()).id }
            var corruptPreview = true
            val replies =
                object : HarvestCircleActualRuntime by runtime {
                    override suspend fun actualDeparturePreview(
                        operation: OperationId,
                        input: HubActualDepartureInput,
                    ): HubActualDeparturePreview {
                        val value = runtime.actualDeparturePreview(operation, input)
                        return if (corruptPreview) {
                            value.copy(
                                proposed = value.proposed.copy(recipient = value.proposed.recipient.copy(id = "fe".repeat(16))),
                            )
                        } else {
                            value
                        }
                    }

                    override suspend fun reconcileActual(
                        context: RequestContext,
                        input: HubActualDepartureInput,
                    ): HubActualReviewReceipt {
                        val value = runtime.reconcileActual(context, input)
                        val original = value.original as HubActualReviewOriginal.Departure
                        return value.copy(
                            original =
                                original.copy(
                                    original = original.original.copy(occurrence = HubOccurrence(HubOccurrenceKind.Unknown)),
                                ),
                        )
                    }
                }
            setContent { NativeActualForm(runtime, replies, hubId, { it.beginReport() }) { presenter = it } }
            waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null }
            onNodeWithTag(
                "actual-observation",
            ).performScrollTo().performTextInput("Reported two departed; source and time require separate review")
            onNodeWithTag("actual-quantity").performScrollTo().performTextInput("2")
            onNodeWithTag("actual-confirm").performScrollTo().performClick()
            onNodeWithTag("actual-submit").performScrollTo().performClick()
            waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is ActualSubmission.Recorded }
            val report =
                assertIs<ActualResult.Reported>(
                    assertIs<ActualSubmission.Recorded>(presenter.state.value.submission).result,
                ).value
            assertEquals(beforePacks, runBlocking { runtime.group(groupId) }.group.wholePacks)
            onNodeWithTag("actual-done").performScrollTo().performClick()
            waitUntil(timeoutMillis = 10_000) { presenter.state.value.page != null && !presenter.state.value.loading }
            onNodeWithTag("actual-review-${report.id}").performScrollTo().performClick()

            fun context() {
                onNodeWithTag("actual-refresh-context").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.currentContext != null && !presenter.state.value.loading }
                onNodeWithTag("actual-accept-context").performScrollTo().performClick()
            }
            context()
            onNodeWithTag("actual-group-$groupId").performScrollTo().performClick()
            context()
            val source =
                assertNotNull(presenter.state.value.draft).context.sources.single {
                    it.operation == hub.receipt.operationId &&
                        it.groupId == groupId
                }
            onNodeWithTag("actual-birth-${source.key}").performScrollTo().performClick()
            onNodeWithTag("actual-recipient").performScrollTo().performClick()
            onNodeWithTag("actual-recipient-${recipient.id}").performScrollTo().performClick()
            onNodeWithTag("actual-occurrence-kind").performScrollTo().performClick()
            onNodeWithText("Known", useUnmergedTree = true).performClick()
            onNodeWithTag(
                "actual-occurrence-start",
            ).performScrollTo().performTextInput(if (beforeHold) "2023-11-14T22:12:40Z" else "2023-11-14T22:13:10Z")
            for ((tag, text) in listOf(
                "source" to "Original physical receipt cohort verified",
                "quantity" to "Two original whole crates observed",
                "recipient" to "Recipient acknowledged these two crates",
                "timing" to "Witnessed actual UTC occurrence",
            )) {
                onNodeWithTag("actual-$tag-evidence").performScrollTo().performTextInput(text)
            }
            onNodeWithTag(
                "actual-duplicate-basis",
            ).performScrollTo().performTextInput(
                "Checked complete known departure history; this was a separate unrecorded physical event",
            )
            onNodeWithTag("actual-confirm").performScrollTo().assertIsNotEnabled()
            onNodeWithTag("actual-preview").performScrollTo().performClick()
            waitUntil(timeoutMillis = 10_000) { presenter.state.value.message != null && !presenter.state.value.loading }
            assertNull(presenter.state.value.preview)
            onNodeWithTag("actual-confirm").performScrollTo().assertIsNotEnabled()
            corruptPreview = false
            onNodeWithTag("actual-preview").performScrollTo().performClick()
            waitUntil(timeoutMillis = 10_000) { presenter.state.value.preview != null && !presenter.state.value.loading }
            val preview = assertIs<ActualPreview.Departure>(presenter.state.value.preview).value
            assertEquals(beforePacks, runBlocking { runtime.group(groupId) }.group.wholePacks)
            assertEquals(afterPacks, preview.proposed.onHandAfterWholePacks)
            assertNull(preview.report.reported.recipientId)
            assertEquals(HubOccurrenceKind.Unknown, preview.report.reported.occurrence.kind)
            if (separateCohort) {
                val assessment = assertIs<HubActualHoldAssessment.Unresolved>(preview.historicalHolds.single())
                assertEquals(HubActualHoldTiming.EstablishedBreach, assessment.conditionalTiming)
                onNodeWithText("Unresolved scope", substring = true).performScrollTo().assertExists()
            } else {
                assertEquals(
                    if (beforeHold) HubActualHoldTiming.BeforeHold else HubActualHoldTiming.EstablishedBreach,
                    assertIs<HubActualHoldAssessment.Verified>(preview.historicalHolds.single()).timing,
                )
            }
            onNodeWithTag("actual-confirm").performScrollTo().performClick()
            onNodeWithTag("actual-submit").performScrollTo().performClick()
            waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is ActualSubmission.Uncertain }
            val operation = assertIs<ActualSubmission.Uncertain>(presenter.state.value.submission).attempt.operation
            assertEquals(afterPacks, runBlocking { runtime.group(groupId) }.group.wholePacks)
            onNodeWithTag("actual-check-original").performScrollTo().performClick()
            waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is ActualSubmission.Recorded }
            assertEquals(operation, assertIs<ActualSubmission.Recorded>(presenter.state.value.submission).attempt.operation)
            val receipt =
                assertIs<ActualResult.Reviewed>(
                    assertIs<ActualSubmission.Recorded>(presenter.state.value.submission).result,
                ).value
            val original = assertIs<HubActualReviewOriginal.Departure>(receipt.original)
            assertEquals(preview.proposed, original.original)
            assertEquals(afterPacks, runBlocking { runtime.group(groupId) }.group.wholePacks)
            val work =
                runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }
                    .rows
                    .single()
                    .work.quantity
            assertEquals(0U, work.completed)
            assertEquals(2U, work.noLongerRequired)
            assertEquals(18U, work.remaining)
            val heldAfter = runBlocking { runtime.treatment(HubTreatmentScope(hub.receipt.trace.id)) }
            assertEquals(held.holds.single().hold, heldAfter.holds.single().hold)
            assertEquals(afterPacks, heldAfter.holds.single().currentWholePacks)
            assertFalse(original.historicalHolds.isEmpty())
        } finally {
            runtime.close()
            Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
        }
    }
}

@Composable
private fun NativeActualForm(
    runtime: TestBridgeHarvestCircleRuntime,
    actual: HarvestCircleActualRuntime,
    hub: String,
    begin: (ActualPresenter) -> Unit,
    ready: (ActualPresenter) -> Unit,
) {
    val snapshot = runtime.currentSnapshot()
    val scope = rememberCoroutineScope()
    val presenter =
        remember(runtime, snapshot.runtimeSession, hub) {
            ActualPresenter(
                actual,
                runtime,
                runtime,
                runtime,
                runtime,
                scope,
                OperationIdSource {
                    OperationId.from(org.harvestcircle.ffi.generateOperationIdV7())
                },
            )
        }
    DisposableEffect(presenter) { onDispose { presenter.close() } }
    LaunchedEffect(presenter) {
        presenter.attach(hub, requireNotNull(snapshot.runtimeSession))
        begin(presenter)
    }
    ready(presenter)
    val state by presenter.state.collectAsState()
    HarvestCircleTheme(AppearanceState()) { ActualWorkspace(state, presenter) { presenter.submit(runtime.currentSnapshot().revision) } }
}
