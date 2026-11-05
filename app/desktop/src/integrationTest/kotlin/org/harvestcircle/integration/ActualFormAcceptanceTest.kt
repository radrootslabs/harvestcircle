package org.harvestcircle.integration

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.v2.runDesktopComposeUiTest
import kotlinx.coroutines.runBlocking
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.application.ActualLookupKind
import org.harvestcircle.application.ActualOriginalLookup
import org.harvestcircle.application.ActualPresenter
import org.harvestcircle.application.ActualPreview
import org.harvestcircle.application.ActualResult
import org.harvestcircle.application.ActualSubmission
import org.harvestcircle.application.ActualViewState
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.HarvestCircleActualRuntime
import org.harvestcircle.application.HubActualDepartureInput
import org.harvestcircle.application.HubActualDeparturePreview
import org.harvestcircle.application.HubActualHoldAssessment
import org.harvestcircle.application.HubActualHoldTiming
import org.harvestcircle.application.HubActualLinkInput
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
import org.harvestcircle.application.HubTraceSnapshotQuery
import org.harvestcircle.application.HubTreatmentScope
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.OperationIdSource
import org.harvestcircle.application.RequestContext
import org.harvestcircle.application.toModel
import org.harvestcircle.testbridge.ffi.actualLinkInterpretationEncodingProbe
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
    fun encodingOnlyLinkPanelSeparatesOriginalQuantityFromZeroMeaningAndUnknownRecipient() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val original = actualLinkInterpretationEncodingProbe().toModel()
            val meaning = assertNotNull(original.interpretation)
            val result =
                ActualResult.Reviewed(
                    HubActualReviewReceipt(
                        original.operationId,
                        meaning.stockRevision,
                        meaning.catalogRevision,
                        HubActualReviewOriginal.Duplicate(original),
                    ),
                )
            setContent {
                val scope = rememberCoroutineScope()
                val presenter =
                    remember(scope) {
                        ActualPresenter(
                            null,
                            null,
                            null,
                            null,
                            null,
                            scope,
                            OperationIdSource { error("Encoding-only display creates no operation") },
                        )
                    }
                DisposableEffect(presenter) { onDispose { presenter.close() } }
                HarvestCircleTheme(AppearanceState()) {
                    ActualWorkspace(
                        ActualViewState(originalLookup = ActualOriginalLookup(ActualLookupKind.Review, original.operationId.value, result)),
                        presenter,
                    ) { error("Encoding-only display submits no mutation") }
                }
            }
            onNodeWithText("Meaning at link review · 0 whole packs").performScrollTo().assertExists()
            onNodeWithText("Recipient at link review: Unknown").performScrollTo().assertExists()
            onNodeWithText("${original.known.variant.value.label} · Original recipient ${original.known.recipient.value.label}")
                .performScrollTo()
                .assertExists()
        }

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

    @Test
    fun staleCatalogRequiresReviewedCurrentContextBeforeAnotherReconciliation() = physicalReview(false, staleCatalog = true)

    @Test
    fun depletedPopulationLinkHasZeroEffectAndReadsItsOriginalAfterRestart() =
        runDesktopComposeUiTest(width = 1280, height = 800) {
            val root = Files.createTempDirectory("harvestcircle-actual-link-form-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            lateinit var presenter: ActualPresenter
            var epoch by mutableStateOf(0)
            var lookup by mutableStateOf(false)
            var operationText = ""
            var writes = 0
            try {
                val hub = runBlocking { HandlingFixture.open(runtime, HubOccurrence(HubOccurrenceKind.Known, 1_699_999_940L)) }
                val known =
                    runBlocking {
                        val catalog = runtime.references(HubReferenceQuery())
                        val recipient =
                            runtime
                                .editReference(
                                    hub.context(),
                                    HubReferenceEdit(
                                        catalog.catalogRevision,
                                        value = HubReferenceValue(HubReferenceKind.Party, "Fictional whole-population recipient"),
                                    ),
                                ).current
                        val report =
                            runtime.reportActual(
                                hub.context(),
                                HubActualReportInput(wholePacks = 20U, observation = "Twenty reported departed; facts reviewed separately"),
                            )
                        val group = runtime.group(hub.receipt.initialPosition.groupId)
                        runtime.reconcileActual(
                            hub.context(),
                            HubActualDepartureInput(
                                report.id,
                                group.stockRevision,
                                group.catalogRevision,
                                group.group.id,
                                group.group.revision,
                                20U,
                                recipient.id,
                                HubOccurrence(HubOccurrenceKind.Known, 1_699_999_990L),
                                hub.receipt.operationId,
                                group.group.id,
                                "Original birth cohort verified",
                                "All twenty original crates",
                                "Recipient acknowledged all twenty",
                                "Witnessed physical occurrence",
                                "Checked complete history; distinct physical event",
                            ),
                        )
                    }
                val duplicate =
                    runBlocking {
                        runtime.reportActual(
                            hub.context(),
                            HubActualReportInput(observation = "Another report may describe the same twenty; no new removal authorized"),
                        )
                    }
                val hubId = runBlocking { assertNotNull(runtime.hubContext()).id }
                val before = runBlocking { runtime.stock(HubStockQuery()) }
                assertTrue(before.rows.isEmpty())
                val work = runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }.rows
                val lost =
                    object : HarvestCircleActualRuntime by runtime {
                        override suspend fun linkActual(
                            context: RequestContext,
                            input: HubActualLinkInput,
                        ): HubActualReviewReceipt {
                            writes++
                            runtime.linkActual(context, input)
                            error("Test-owned lost link reply after native commit")
                        }
                    }
                setContent {
                    key(epoch) {
                        NativeActualForm(runtime, lost, hubId, {
                            if (lookup) {
                                it.beginOriginalLookup()
                                it.editOriginalLookup(ActualLookupKind.Review, operationText)
                                it.lookupOriginal()
                            } else {
                                it.beginLink(duplicate)
                            }
                        }) { presenter = it }
                    }
                }
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.draft != null }

                fun context() {
                    onNodeWithTag("actual-refresh-context").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { presenter.state.value.currentContext != null && !presenter.state.value.loading }
                    onNodeWithTag("actual-accept-context").performScrollTo().performClick()
                }
                context()
                onNodeWithTag("actual-trace-${hub.receipt.trace.id}").performScrollTo().performClick()
                context()
                val choice = assertNotNull(presenter.state.value.draft).context.known.single { it.operation == known.operationId }
                onNodeWithTag("actual-known-${choice.key}").performScrollTo().performClick()
                onNodeWithTag(
                    "actual-duplicate-basis",
                ).performScrollTo().performTextInput("Verified both reports refer to the same complete twenty-crate departure")
                onNodeWithTag("actual-preview").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.preview != null && !presenter.state.value.loading }
                val preview = assertIs<ActualPreview.Link>(presenter.state.value.preview)
                assertEquals(20U, preview.value.proposed.known.wholePacks)
                assertEquals(
                    20U,
                    preview.value.proposed.known.lines
                        .single()
                        .wholePacks,
                )
                assertEquals(before, runBlocking { runtime.stock(HubStockQuery()) })
                onNodeWithTag("actual-confirm").performScrollTo().performClick()
                onNodeWithTag("actual-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is ActualSubmission.Uncertain }
                runOnIdle {
                    presenter.preview()
                    presenter.submit(runtime.currentSnapshot().revision)
                }
                assertEquals(1, writes)
                onNodeWithTag("actual-check-original").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is ActualSubmission.Recorded }
                val receipt =
                    assertIs<ActualResult.Reviewed>(
                        assertIs<ActualSubmission.Recorded>(presenter.state.value.submission).result,
                    ).value
                assertEquals(preview.attempt.operation, receipt.operationId)
                assertEquals(preview.value.proposed, assertIs<HubActualReviewOriginal.Duplicate>(receipt.original).original)
                assertTrue(runBlocking { runtime.stock(HubStockQuery()) }.rows.isEmpty())
                assertEquals(work, runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }.rows)
                assertEquals(
                    20U,
                    runBlocking { runtime.traceSnapshot(HubTraceSnapshotQuery(hub.receipt.trace.id)) }.actualDepartedWholePacks,
                )
                val oldPresenter = presenter
                val oldBinding = runtime.currentSnapshot().runtimeSession
                runOnIdle {
                    oldPresenter.close()
                    runBlocking {
                        runtime.restart()
                        runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                    }
                    operationText = receipt.operationId.value
                    lookup = true
                    epoch++
                }
                waitUntil(timeoutMillis = 10_000) {
                    presenter !== oldPresenter &&
                        presenter.state.value.originalLookup
                            ?.result != null
                }
                assertTrue(oldBinding != runtime.currentSnapshot().runtimeSession)
                assertFalse(oldPresenter.state.value.available)
                assertEquals(
                    receipt,
                    assertIs<ActualResult.Reviewed>(
                        presenter.state.value.originalLookup
                            ?.result,
                    ).value,
                )
                onNodeWithText("Linked to a known departure · Zero new stock or work effect").performScrollTo().assertExists()
                assertEquals(known, runBlocking { runtime.actualReviewOperation(known.operationId) })
                assertEquals(1, writes)
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }

    @Test
    fun unknownHoldTimeRemainsUnknownInTheNativeFormWithoutChangingTheOriginalClocks() = physicalReview(false, unknownHoldTime = true)

    private fun physicalReview(
        beforeHold: Boolean,
        separateCohort: Boolean = false,
        staleCatalog: Boolean = false,
        unknownHoldTime: Boolean = false,
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
                            if (unknownHoldTime) {
                                HubOccurrence(
                                    HubOccurrenceKind.Unknown,
                                )
                            } else {
                                HubOccurrence(HubOccurrenceKind.Known, 1_699_999_970L)
                            },
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
            var preview = assertIs<ActualPreview.Departure>(presenter.state.value.preview).value
            assertEquals(beforePacks, runBlocking { runtime.group(groupId) }.group.wholePacks)
            assertEquals(afterPacks, preview.proposed.onHandAfterWholePacks)
            assertNull(preview.report.reported.recipientId)
            assertEquals(HubOccurrenceKind.Unknown, preview.report.reported.occurrence.kind)
            if (unknownHoldTime) {
                val assessment = assertIs<HubActualHoldAssessment.Verified>(preview.historicalHolds.single())
                assertEquals(HubActualHoldTiming.TimingUnknown, assessment.timing)
                assertEquals(HubOccurrenceKind.Unknown, assessment.original.stamp.occurrence.kind)
                assertEquals(report, preview.report)
                onNodeWithText("Verified historical scope · TimingUnknown", substring = true).performScrollTo().assertExists()
                onAllNodesWithText("EstablishedBreach", substring = true).assertCountEquals(0)
                onAllNodesWithText("BeforeHold", substring = true).assertCountEquals(0)
            } else if (separateCohort) {
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
            if (staleCatalog) {
                runBlocking {
                    val catalog = runtime.references(HubReferenceQuery())
                    runtime.editReference(
                        hub.context(),
                        HubReferenceEdit(
                            catalog.catalogRevision,
                            value = HubReferenceValue(HubReferenceKind.Party, "Another fictional party"),
                        ),
                    )
                }
                onNodeWithTag("actual-submit").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.submission is ActualSubmission.Rejected }
                val rejected = assertIs<ActualSubmission.Rejected>(presenter.state.value.submission).attempt.operation
                assertEquals(beforePacks, runBlocking { runtime.group(groupId) }.group.wholePacks)
                assertNull(runBlocking { runtime.actualReviewOperation(rejected) })
                onNodeWithTag("actual-review-rejected").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.currentContext != null && !presenter.state.value.loading }
                onNodeWithTag("actual-accept-context").performScrollTo().performClick()
                onNodeWithTag("actual-birth-${source.key}").performScrollTo().performClick()
                onNodeWithTag("actual-submit").performScrollTo().assertIsNotEnabled()
                onNodeWithTag("actual-preview").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { presenter.state.value.preview != null && !presenter.state.value.loading }
                val fresh = assertIs<ActualPreview.Departure>(presenter.state.value.preview)
                assertTrue(fresh.attempt.operation != rejected)
                preview = fresh.value
                onNodeWithTag("actual-confirm").performScrollTo().performClick()
            }
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
            if (unknownHoldTime) assertEquals(report, original.report)
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
