package org.harvestcircle.integration

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsOff
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
import org.harvestcircle.application.HarvestCircleApplicationWithDependencies
import org.harvestcircle.application.HarvestCirclePresenter
import org.harvestcircle.application.HubAcceptanceStatus
import org.harvestcircle.application.HubDepartureInput
import org.harvestcircle.application.HubDepartureReviewInput
import org.harvestcircle.application.HubDispatchActionInput
import org.harvestcircle.application.HubDispatchInput
import org.harvestcircle.application.HubDispatchLineInput
import org.harvestcircle.application.HubDispatchMaterialInput
import org.harvestcircle.application.HubDispatchReceipt
import org.harvestcircle.application.HubDispatchState
import org.harvestcircle.application.HubHandlingInput
import org.harvestcircle.application.HubHandlingQuery
import org.harvestcircle.application.HubIdentificationStatus
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.OperationId
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
import kotlin.test.assertNotNull

@OptIn(ExperimentalTestApi::class)
class DepartureAcceptanceTest {
    @Test
    fun twoRecipientsAndMultilineDepartureRemainAtomicThroughNativeBindings() =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-departure-lines-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            try {
                val hub = HandlingFixture.open(runtime)
                val group = runtime.group(hub.receipt.initialPosition.groupId)
                runtime.applyHandling(
                    hub.context(),
                    HubHandlingInput.SetAcceptance(
                        group.stockRevision,
                        group.catalogRevision,
                        listOf(group.group.id),
                        HubAcceptanceStatus.Accepted,
                        HubOccurrence(HubOccurrenceKind.Unknown),
                        "Reviewed all twenty sample crates",
                    ),
                )
                runtime.applyHandling(hub.context(), hub.move(runtime.group(group.group.id), hub.a3, 20U))
                val six = hub.split(group.group.id, 6U, "Six for A")
                val two = hub.split(group.group.id, 2U, "First two for B")
                val anotherTwo = hub.split(group.group.id, 2U, "Second two for B")
                val a = issue(hub, listOf(six), "Fictional recipient A")
                val b = issue(hub, listOf(two, anotherTwo), "Fictional recipient B")
                val originalA = runtime.applyDeparture(hub.context(), departureInput(runtime, a))
                assertEquals(14UL, runtime.stock(HubStockQuery()).rows.sumOf { it.position.wholePacks.toULong() })
                val hold = hub.hold(listOf(anotherTwo), "Precautionary hold blocks every line of B")
                val before = runtime.stock(HubStockQuery())
                val blocked =
                    runtime.departureReview(
                        HubDepartureReviewInput(
                            b.record.id,
                            b.record.revision,
                            b.record.versions
                                .last()
                                .id,
                        ),
                    )
                assertEquals(false, blocked.ready)
                assertFailsWith<ApplicationFailure> { runtime.applyDeparture(hub.context(), departureInput(runtime, b)) }
                assertEquals(before, runtime.stock(HubStockQuery()))
                assertEquals(HubDispatchState.Issued, runtime.dispatchRecord(b.record.id).state)
                hub.release(hold.operationId)
                val originalB = runtime.applyDeparture(hub.context(), departureInput(runtime, b))
                assertEquals(2, originalB.lines.size)
                assertEquals(4UL, originalB.lines.sumOf { it.wholePacks.toULong() })
                val after = runtime.stock(HubStockQuery())
                assertEquals(10UL, after.rows.sumOf { it.position.wholePacks.toULong() })
                val work =
                    runtime
                        .work(HubHandlingQuery(openOnly = false))
                        .rows
                        .single()
                        .work.quantity
                assertEquals(listOf(20U, 20U, 0U, 0U), listOf(work.required, work.completed, work.noLongerRequired, work.remaining))
                assertEquals(a.record, originalA.paper)
                assertEquals(b.record, originalB.paper)
                runtime.restart()
                runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                assertEquals(after, runtime.stock(HubStockQuery()))
                assertEquals(originalA, runtime.departureOperation(originalA.operationId))
                assertEquals(originalB, runtime.departureOperation(originalB.operationId))
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }

    @Test
    fun confirmedActualDepartureRetiresOnlyPendingWorkAndPreservesIssuedFactsAcrossRestart() =
        runDesktopComposeUiTest(width = 1100, height = 720) {
            val root = Files.createTempDirectory("harvestcircle-departure-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            var showApplication by mutableStateOf(true)
            var closeRequested by mutableStateOf(false)
            var approvedExits = 0
            val ids = OperationIdSource { OperationId.from(org.harvestcircle.ffi.generateOperationIdV7()) }
            try {
                val hub = runBlocking { HandlingFixture.open(runtime) }
                val issued =
                    runBlocking {
                        val original = runtime.group(hub.receipt.initialPosition.groupId)
                        runtime.applyHandling(hub.context(), hub.move(original, hub.a3, 8U))
                        val five = hub.split(original.group.id, 5U, "Purple stack")
                        val group = runtime.group(five)
                        runtime.applyHandling(
                            hub.context(),
                            HubHandlingInput.SetAcceptance(
                                group.stockRevision,
                                group.catalogRevision,
                                listOf(five),
                                HubAcceptanceStatus.Accepted,
                                HubOccurrence(HubOccurrenceKind.Unknown),
                                "Sample acceptance reviewed for five identified crates",
                            ),
                        )
                        val catalog = runtime.references(HubReferenceQuery())
                        val recipient =
                            runtime
                                .editReference(
                                    hub.context(),
                                    HubReferenceEdit(
                                        catalog.catalogRevision,
                                        value = HubReferenceValue(HubReferenceKind.Party, "Fictional recipient A"),
                                    ),
                                ).current
                        val current = runtime.group(five)
                        val prepared =
                            runtime.applyDispatch(
                                hub.context(),
                                HubDispatchInput.Prepare(
                                    HubDispatchMaterialInput(
                                        current.stockRevision,
                                        current.catalogRevision,
                                        recipient.id,
                                        listOf(HubDispatchLineInput(five, current.group.revision, 5U)),
                                        "Reviewed five original crates and recipient A",
                                    ),
                                ),
                            )
                        val view = runtime.dispatchRecord(prepared.record.id)
                        runtime.applyDispatch(
                            hub.context(),
                            HubDispatchInput.Issue(
                                HubDispatchActionInput(
                                    view.stockRevision,
                                    view.catalogRevision,
                                    prepared.record.id,
                                    prepared.record.revision,
                                    "As-issued identification for five crates to recipient A",
                                ),
                            ),
                        )
                    }
                val before = runBlocking { runtime.stock(HubStockQuery()) }
                val workBefore =
                    runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }
                        .rows
                        .single()
                        .work.quantity
                assertEquals(
                    listOf(20U, 8U, 0U, 12U),
                    listOf(workBefore.required, workBefore.completed, workBefore.noLongerRequired, workBefore.remaining),
                )
                val originalIssue = runBlocking { runtime.dispatchOperation(issued.operationId) }
                setContent {
                    HarvestCircleWindowChromeEnvironment(HarvestCircleWindowChromeExclusion.None) {
                        if (showApplication) {
                            HarvestCircleApplicationWithDependencies(
                                closeRequested = closeRequested,
                                onExitApproved = { approvedExits += 1 },
                                clipboardFactory = { scope ->
                                    SecretClipboardController(scope, DepartureClipboard(), clearDelayMillis = 60_000)
                                },
                            ) { scope -> HarvestCirclePresenter(runtime, scope, ApplicationClock { UnixSeconds(1_700_000_000) }, ids) }
                        }
                    }
                }
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("sidebar-HubDispatch").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("sidebar-HubDispatch").performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { runCatching { onNodeWithTag("dispatch-record-${issued.record.id}").assertIsEnabled() }.isSuccess }
                onNodeWithTag("dispatch-record-${issued.record.id}").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("dispatch-depart").assertIsEnabled() }.isSuccess }
                onNodeWithTag("dispatch-depart").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("departure-confirm").assertIsEnabled() }.isSuccess }
                onNodeWithTag("departure-save").assertIsNotEnabled()
                onNodeWithTag(
                    "departure-observation",
                ).performScrollTo().performTextInput("Five Purple stack crates actually left for recipient A")
                onNodeWithTag("departure-save").assertIsNotEnabled()
                onNodeWithTag("departure-confirm").performClick()
                onNodeWithTag("departure-keep").performClick()
                onNodeWithTag("departure-resume").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("departure-accept-review").assertIsEnabled() }.isSuccess }
                onNodeWithTag("departure-confirm").assertIsOff()
                onNodeWithTag("departure-save").assertIsNotEnabled()
                onNodeWithTag("departure-accept-review").performScrollTo().performClick()
                onNodeWithText("Five Purple stack crates actually left for recipient A").performScrollTo().assertIsDisplayed()
                assertEquals(before, runBlocking { runtime.stock(HubStockQuery()) })
                onNodeWithTag("departure-confirm").performClick()
                onNodeWithTag("departure-save").performClick()
                waitUntil(timeoutMillis = 10_000) { onNodeWithTag("departure-recorded").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("departure-recorded").assertIsDisplayed()
                val after = runBlocking { runtime.stock(HubStockQuery()) }
                assertEquals(15UL, after.rows.sumOf { it.position.wholePacks.toULong() })
                val workAfter =
                    runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }
                        .rows
                        .single()
                        .work.quantity
                assertEquals(
                    listOf(20U, 8U, 5U, 7U),
                    listOf(workAfter.required, workAfter.completed, workAfter.noLongerRequired, workAfter.remaining),
                )
                val used = runBlocking { runtime.dispatchRecord(issued.record.id) }
                assertEquals(HubDispatchState.Departed, used.state)
                assertEquals(issued.record, used.record)
                assertEquals(
                    HubIdentificationStatus.Issued,
                    used.record.versions
                        .single()
                        .status,
                )
                val operation = assertNotNull(used.departure).operationId
                val original = assertNotNull(runBlocking { runtime.departureOperation(operation) })
                assertEquals("Five Purple stack crates actually left for recipient A", original.observation)
                assertEquals(issued.record, original.paper)
                assertEquals(5U, original.lines.single().wholePacks)
                onNodeWithTag("departure-done").performScrollTo().performClick()
                waitUntil(timeoutMillis = 10_000) {
                    onNodeWithText("Departed · Record revision ${issued.record.revision}").runCatching { fetchSemanticsNode() }.isSuccess
                }
                closeRequested = true
                waitUntil(timeoutMillis = 10_000) { approvedExits == 1 }
                showApplication = false
                waitForIdle()
                runBlocking {
                    runtime.restart()
                    runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                    assertEquals(after, runtime.stock(HubStockQuery()))
                    assertEquals(
                        workAfter,
                        runtime
                            .work(HubHandlingQuery(openOnly = false))
                            .rows
                            .single()
                            .work.quantity,
                    )
                    assertEquals(used, runtime.dispatchRecord(issued.record.id))
                    assertEquals(original, runtime.departureOperation(operation))
                    assertEquals(originalIssue, runtime.dispatchOperation(issued.operationId))
                    assertEquals(hub.receipt, runtime.receiptOperation(hub.receipt.operationId))
                    runtime.execute(ApplicationCommand.SignOut)
                    assertFailsWith<ApplicationFailure> { runtime.departureOperation(operation) }
                }
            } finally {
                showApplication = false
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }
}

private suspend fun issue(
    hub: HandlingFixture,
    groups: List<String>,
    recipientName: String,
): HubDispatchReceipt {
    val runtime = hub.runtime
    val catalog = runtime.references(HubReferenceQuery())
    val recipient =
        runtime
            .editReference(
                hub.context(),
                HubReferenceEdit(
                    catalog.catalogRevision,
                    value = HubReferenceValue(HubReferenceKind.Party, recipientName),
                ),
            ).current
    val source = groups.map { runtime.group(it) }
    val prepared =
        runtime.applyDispatch(
            hub.context(),
            HubDispatchInput.Prepare(
                HubDispatchMaterialInput(
                    source.first().stockRevision,
                    source.first().catalogRevision,
                    recipient.id,
                    source.map { HubDispatchLineInput(it.group.id, it.group.revision, it.group.wholePacks) },
                    "Reviewed actual original crates and recipient",
                ),
            ),
        )
    val view = runtime.dispatchRecord(prepared.record.id)
    return runtime.applyDispatch(
        hub.context(),
        HubDispatchInput.Issue(
            HubDispatchActionInput(
                view.stockRevision,
                view.catalogRevision,
                prepared.record.id,
                prepared.record.revision,
                "Reviewed issued sample identification",
            ),
        ),
    )
}

private suspend fun departureInput(
    runtime: TestBridgeHarvestCircleRuntime,
    paper: HubDispatchReceipt,
): HubDepartureInput {
    val view = runtime.dispatchRecord(paper.record.id)
    return HubDepartureInput(
        view.stockRevision,
        view.catalogRevision,
        paper.record.id,
        paper.record.revision,
        paper.record.versions
            .last()
            .id,
        HubOccurrence(HubOccurrenceKind.Unknown),
        "Observed actual departure for the issued recipient",
    )
}

private class DepartureClipboard : TextClipboard {
    private var value: String? = null

    override fun readText(): String? = value

    override fun writeText(value: String) {
        this.value = value.ifEmpty { null }
    }
}
