package org.harvestcircle.integration

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.onAllNodesWithTag
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
import org.harvestcircle.application.HubHandlingQuery
import org.harvestcircle.application.HubHandlingResult
import org.harvestcircle.application.HubLineageQuery
import org.harvestcircle.application.HubLocationPurpose
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubTreatmentChange
import org.harvestcircle.application.HubTreatmentScope
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
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class ReceivingAcceptanceTest {
    @Test
    fun receivingAndHandlingUseNativeStorageAndSurviveRestart() =
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
                val alternate = add(HubReferenceValue(HubReferenceKind.Location, "B3", locationPurpose = HubLocationPurpose.Storage))
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
                val groupId =
                    first.rows
                        .single()
                        .position.groupId

                fun openGroup(id: String) {
                    onNodeWithTag("sidebar-HubStock").performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("stock-inspect-$id").runCatching { fetchSemanticsNode() }.isSuccess }
                    onNodeWithTag("stock-inspect-$id").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { runCatching { onNodeWithTag("stock-handling-$id").assertIsEnabled() }.isSuccess }
                    onNodeWithTag("stock-handling-$id").performScrollTo().performClick()
                    waitUntil(
                        timeoutMillis = 10_000,
                    ) { onNodeWithTag("handling-begin-move").runCatching { fetchSemanticsNode() }.isSuccess }
                }

                fun destination(id: String) {
                    onNodeWithTag("handling-target").performScrollTo().performClick()
                    onNodeWithTag("handling-target-$id").performScrollTo().performClick()
                }

                fun record(basis: String) {
                    onNodeWithTag("handling-observation").performScrollTo().performTextInput(basis)
                    onNodeWithTag("handling-save").performScrollTo().performClick()
                    waitUntil(timeoutMillis = 10_000) { onNodeWithTag("handling-recorded").runCatching { fetchSemanticsNode() }.isSuccess }
                }

                fun done() = onNodeWithTag("handling-done").performScrollTo().performClick()
                openGroup(groupId)
                onNodeWithTag("handling-begin-move").performScrollTo().performClick()
                onNodeWithTag("handling-portion").performScrollTo().performClick()
                onNodeWithTag("handling-quantity").performScrollTo().performTextInput("8")
                onNodeWithTag("handling-physical-label").performScrollTo().performTextInput("Blue tagged stack")
                onNodeWithTag("handling-physical-observation").performScrollTo().performTextInput("Eight separately tagged crates")
                destination(planned)
                record("Manually confirmed eight crates arrived at A3")
                val moved = runBlocking { runtime.stock(HubStockQuery()) }
                assertEquals(
                    12U,
                    moved.rows
                        .single { it.position.groupId == groupId }
                        .position.wholePacks,
                )
                val child =
                    moved.rows
                        .single { it.position.groupId != groupId }
                        .position.groupId
                assertEquals(
                    8U,
                    moved.rows
                        .single { it.position.groupId == child }
                        .position.wholePacks,
                )
                assertEquals(
                    planned,
                    moved.rows
                        .single { it.position.groupId == child }
                        .position.locationId,
                )
                val intakeWork = runBlocking { runtime.work(HubHandlingQuery()) }.rows.single()
                assertEquals(8U, intakeWork.work.quantity.completed)
                assertEquals(12U, intakeWork.work.quantity.remaining)
                done()
                onNodeWithTag("sidebar-HubWork").performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { onNodeWithTag("work-inspect-${intakeWork.work.id}").runCatching { fetchSemanticsNode() }.isSuccess }
                onAllNodesWithTag("work-row-${intakeWork.work.id}").assertCountEquals(1)
                onNodeWithTag("work-inspect-${intakeWork.work.id}").performScrollTo().performClick()
                onNodeWithTag("handling-begin-replan").performScrollTo().performClick()
                destination(alternate)
                record("Plan remaining twelve for B3 without moving stock")
                val replanned = runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }
                val old =
                    replanned.rows
                        .single { it.work.id == intakeWork.work.id }
                        .work.quantity
                assertEquals(8U, old.completed)
                assertEquals(12U, old.noLongerRequired)
                assertEquals(0U, old.remaining)
                assertEquals(moved.rows.map { it.position }, runBlocking { runtime.stock(HubStockQuery()) }.rows.map { it.position })
                done()
                openGroup(groupId)
                onNodeWithTag("handling-begin-discrepancy").performScrollTo().performClick()
                record("Inspect outer packaging; retain this observation")
                val issue = runBlocking { runtime.discrepancies(HubHandlingQuery()) }.rows.single()
                done()
                onNodeWithTag("sidebar-HubWork").performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { onNodeWithTag("handling-discrepancies-tab").runCatching { fetchSemanticsNode() }.isSuccess }
                waitUntil(
                    timeoutMillis = 10_000,
                ) { runCatching { onNodeWithTag("handling-discrepancies-tab").assertIsEnabled() }.isSuccess }
                onNodeWithTag("handling-discrepancies-tab").performScrollTo().performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { onNodeWithTag("discrepancy-inspect-${issue.value.id}").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithTag("discrepancy-inspect-${issue.value.id}").performScrollTo().performClick()
                onNodeWithTag("handling-begin-resolve").performScrollTo().performClick()
                record("Packaging inspected; original observation retained")
                assertEquals(0U, runBlocking { runtime.discrepancies(HubHandlingQuery()) }.total)
                val originalIssue = runBlocking { runtime.handlingOperation(issue.createdOperation) }
                assertNull(assertIs<HubHandlingResult.DiscrepancyChanged>(originalIssue?.result).value.resolution)
                assertEquals(replanned.rows, runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }.rows)
                done()
                openGroup(child)
                onNodeWithTag("handling-begin-new-work").performScrollTo().performClick()
                destination(receiving)
                record("Explicitly bring the eight previously put-away crates back to Receiving")
                done()
                openGroup(child)
                onNodeWithTag("handling-begin-move").performScrollTo().performClick()
                destination(receiving)
                record("Manually confirmed the entire identified group arrived at Receiving")
                done()
                val workBeforeSeparation = runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }.rows.map { it.work }
                openGroup(groupId)
                onNodeWithTag("handling-begin-split").performScrollTo().performClick()
                onNodeWithTag("handling-target").assertDoesNotExist()
                onNodeWithTag("handling-quantity").performScrollTo().performTextInput("5")
                onNodeWithTag("handling-physical-label").performScrollTo().performTextInput("Purple five")
                onNodeWithTag(
                    "handling-physical-observation",
                ).performScrollTo().performTextInput("Five crates in a separately tagged container")
                record("Observed five original crates separated at Receiving")
                done()
                val lineage =
                    runBlocking {
                        runtime.lineage(
                            HubLineageQuery(
                                first.rows
                                    .single()
                                    .trace.id,
                            ),
                        )
                    }
                assertEquals(3U, lineage.total)
                val five = lineage.rows.single { it.identification?.label == "Purple five" }
                assertEquals(5U, five.wholePacks)
                assertEquals(receiving, five.location?.id)
                assertEquals(groupId, five.parentId)
                assertEquals(workBeforeSeparation, runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }.rows.map { it.work })
                openGroup(groupId)
                onNodeWithTag("portion-lineage").performScrollTo().assertIsDisplayed()
                onNodeWithTag("portion-open-${five.id}").performScrollTo().performClick()
                waitUntil(
                    timeoutMillis = 10_000,
                ) { onNodeWithText("Physical identifier: Purple five").runCatching { fetchSemanticsNode() }.isSuccess }
                onNodeWithText("Physical identifier: Purple five").performScrollTo().assertIsDisplayed()
                onNodeWithTag("treatment-begin-acceptance").performScrollTo().performClick()
                onNodeWithTag("treatment-status-Accepted").performScrollTo().performClick()
                record("Explicit sample acceptance of the purple five")
                done()
                openGroup(five.id)
                onNodeWithTag("treatment-begin-hold").performScrollTo().performClick()
                record("Independent sample hold A on the purple five")
                done()
                openGroup(five.id)
                onNodeWithTag("treatment-begin-hold").performScrollTo().performClick()
                record("Independent sample hold B on the same purple five")
                done()
                val treatmentScope =
                    HubTreatmentScope(
                        first.rows
                            .single()
                            .trace.id,
                    )
                val held = runBlocking { runtime.treatment(treatmentScope) }
                assertEquals(5U, held.summary.restricted)
                assertEquals(0U, held.summary.departureEligible)
                assertEquals(2, held.holds.size)
                held.holds.forEachIndexed { index, row ->
                    openGroup(five.id)
                    onNodeWithTag("treatment-begin-release").performScrollTo().performClick()
                    onNodeWithTag("treatment-hold-${row.hold.stamp.id}").performScrollTo().performClick()
                    record("Explicitly reviewed and released only selected sample hold ${index + 1}")
                    val submitted = runBlocking { runtime.treatment(treatmentScope) }
                    assertEquals(if (index == 0) 5U else 0U, submitted.summary.restricted)
                    assertEquals(if (index == 0) 0U else 5U, submitted.summary.departureEligible)
                    val releaseOperation =
                        assertNotNull(submitted.holds.single { it.hold.stamp.id == row.hold.stamp.id }.releasedOperationId)
                    val originalRelease =
                        assertIs<HubHandlingResult.Treatment>(runBlocking { runtime.handlingOperation(releaseOperation) }?.result)
                    assertEquals(row.hold.stamp.id, assertIs<HubTreatmentChange.HoldReleased>(originalRelease.value.change).holdId)
                    done()
                }
                openGroup(five.id)
                onNodeWithTag("treatment-summary").performScrollTo().assertIsDisplayed()
                val finalTreatment = runBlocking { runtime.treatment(treatmentScope) }
                val finalLineage =
                    runBlocking {
                        runtime.lineage(
                            HubLineageQuery(
                                first.rows
                                    .single()
                                    .trace.id,
                            ),
                        )
                    }
                assertEquals(lineage.rows, finalLineage.rows)
                val finalStock = runBlocking { runtime.stock(HubStockQuery()) }
                assertEquals(20UL, finalStock.rows.sumOf { it.position.wholePacks.toULong() })
                assertTrue(finalStock.rows.all { it.position.locationId == receiving })
                val finalWork = runBlocking { runtime.work(HubHandlingQuery(openOnly = false)) }
                assertEquals(3U, finalWork.total)
                assertEquals(
                    old,
                    finalWork.rows
                        .single { it.work.id == intakeWork.work.id }
                        .work.quantity,
                )
                val originalReceipt = runBlocking { runtime.receiptOperation(intakeWork.createdOperation) }
                assertEquals(20U, originalReceipt?.initialPosition?.wholePacks)
                assertEquals(20U, originalReceipt?.initialPutAway?.remaining)
                closeRequested = true
                waitUntil(timeoutMillis = 10_000) { approvedExits == 1 }
                showApplication = false
                waitForIdle()
                // Reopen the same real database with the test-owned credential custody retained.
                runBlocking {
                    runtime.restart()
                    runtime.execute(ApplicationCommand.ActivateIdentity(owner))
                    val reopened = runtime.stock(HubStockQuery())
                    assertEquals(finalStock, reopened)
                    assertEquals(finalTreatment, runtime.treatment(treatmentScope))
                    assertEquals(
                        finalLineage,
                        runtime.lineage(
                            HubLineageQuery(
                                first.rows
                                    .single()
                                    .trace.id,
                            ),
                        ),
                    )
                    assertEquals(finalWork, runtime.work(HubHandlingQuery(openOnly = false)))
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
