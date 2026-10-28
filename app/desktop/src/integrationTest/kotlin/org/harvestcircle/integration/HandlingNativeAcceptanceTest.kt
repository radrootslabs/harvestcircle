package org.harvestcircle.integration

import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.ApplicationFailure
import org.harvestcircle.application.HubGroupCreationKind
import org.harvestcircle.application.HubGroupView
import org.harvestcircle.application.HubHandlingInput
import org.harvestcircle.application.HubHandlingQuery
import org.harvestcircle.application.HubHandlingResult
import org.harvestcircle.application.HubLineageQuery
import org.harvestcircle.application.HubLocationPurpose
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubPhysicalIdentification
import org.harvestcircle.application.HubReceiptInput
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.HubSourceInput
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubStockReceipt
import org.harvestcircle.application.IdentityId
import org.harvestcircle.application.OperationId
import org.harvestcircle.application.RequestContext
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertNull

class HandlingNativeAcceptanceTest {
    @Test
    fun actualMovementAndOriginalResultsSurviveReturnAndRestart() =
        withHub { hub ->
            val runtime = hub.runtime
            val original = runtime.group(hub.receipt.initialPosition.groupId)
            val input = hub.move(original, hub.a3, 8U)
            val context = hub.context()
            val recorded = runtime.applyHandling(context, input)
            val result = assertIs<HubHandlingResult.Moved>(recorded.result)
            assertEquals(8U, result.wholePacks)
            assertEquals(12U, result.remainingWholePacks)
            assertEquals(hub.owner, recorded.actor)
            assertEquals(recorded, runtime.applyHandling(context, input))
            assertFailsWith<ApplicationFailure> { runtime.applyHandling(hub.context(), input) }
            val arrived = runtime.group(result.movedGroupId)
            assertEquals("Blue stack", arrived.group.identification?.label)
            assertEquals(original.group.id, arrived.group.parentId)
            assertEquals(
                8U,
                runtime
                    .work(HubHandlingQuery())
                    .rows
                    .single()
                    .work.quantity.completed,
            )
            runtime.applyHandling(hub.context(), hub.move(arrived, hub.receiving, 8U))
            val returned = runtime.group(result.movedGroupId)
            runtime.applyHandling(hub.context(), hub.move(returned, hub.a3, 8U))
            val work =
                runtime
                    .work(HubHandlingQuery())
                    .rows
                    .single()
                    .work.quantity
            assertEquals(8U, work.completed)
            assertEquals(12U, work.remaining)
            assertEquals(
                20U,
                runtime
                    .stock(HubStockQuery())
                    .rows
                    .sumOf { it.position.wholePacks.toULong() }
                    .toUInt(),
            )
            assertEquals(recorded, runtime.handlingOperation(recorded.operationId))
            runtime.restart()
            runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
            assertEquals(recorded, runtime.handlingOperation(recorded.operationId))
            assertEquals(
                work,
                runtime
                    .work(HubHandlingQuery())
                    .rows
                    .single()
                    .work.quantity,
            )
            runtime.execute(ApplicationCommand.SignOut)
            assertFailsWith<ApplicationFailure> { runtime.group(result.movedGroupId) }
            assertFailsWith<ApplicationFailure> { runtime.handlingOperation(recorded.operationId) }
        }

    @Test
    fun replanningAndExplicitNewWorkKeepPhysicalStockAndEarlierCompletion() =
        withHub { hub ->
            val runtime = hub.runtime
            val root = runtime.group(hub.receipt.initialPosition.groupId)
            val moved = runtime.applyHandling(hub.context(), hub.move(root, hub.a3, 8U))
            val child = assertIs<HubHandlingResult.Moved>(moved.result).movedGroupId
            val before = runtime.stock(HubStockQuery()).rows.map { it.position }
            val work = runtime.work(HubHandlingQuery())
            val original = work.rows.single().work
            val replacement =
                runtime.applyHandling(
                    hub.context(),
                    HubHandlingInput.Replan(
                        work.stockRevision,
                        work.catalogRevision,
                        original.id,
                        original.quantity.revision,
                        hub.b3,
                        "Remaining twelve are now planned for B3",
                    ),
                )
            val changed = assertIs<HubHandlingResult.WorkChanged>(replacement.result)
            assertEquals(
                8U,
                changed.previous
                    ?.after
                    ?.quantity
                    ?.completed,
            )
            assertEquals(
                12U,
                changed.previous
                    ?.after
                    ?.quantity
                    ?.noLongerRequired,
            )
            assertEquals(12U, changed.replacement.quantity.remaining)
            assertEquals(before, runtime.stock(HubStockQuery()).rows.map { it.position })
            val atA3 = runtime.group(child)
            val newWork =
                runtime.applyHandling(
                    hub.context(),
                    HubHandlingInput.NewWork(
                        atA3.stockRevision,
                        atA3.catalogRevision,
                        listOf(child),
                        hub.receiving,
                        "Explicit new handling of the eight previously put away",
                    ),
                )
            assertEquals(8U, assertIs<HubHandlingResult.WorkChanged>(newWork.result).replacement.quantity.required)
            assertEquals(before, runtime.stock(HubStockQuery()).rows.map { it.position })
            val fresh = runtime.group(child)
            runtime.applyHandling(hub.context(), hub.move(fresh, hub.receiving, 8U))
            val all = runtime.work(HubHandlingQuery(openOnly = false))
            assertEquals(3U, all.total)
            assertEquals(changed.previous?.after, all.rows.single { it.work.id == original.id }.work)
            assertEquals(
                12U,
                all.rows
                    .single { it.work.id == changed.replacement.id }
                    .work.quantity.remaining,
            )
            assertEquals(replacement, runtime.handlingOperation(replacement.operationId))
        }

    @Test
    fun discrepancyFollowUpRetainsOriginalObservationWithoutChangingStockOrWork() =
        withHub { hub ->
            val runtime = hub.runtime
            val group = runtime.group(hub.receipt.initialPosition.groupId)
            val before = runtime.stock(HubStockQuery()).rows
            val work = runtime.work(HubHandlingQuery()).rows
            val opened =
                runtime.applyHandling(
                    hub.context(),
                    HubHandlingInput.OpenDiscrepancy(
                        group.stockRevision,
                        group.catalogRevision,
                        group.trace.id,
                        group.group.id,
                        "Outer packaging requires a recorded inspection",
                    ),
                )
            val issue = assertIs<HubHandlingResult.DiscrepancyChanged>(opened.result).value
            val page = runtime.discrepancies(HubHandlingQuery())
            assertEquals(1U, page.total)
            assertEquals(hub.owner, page.rows.single().createdBy)
            runtime.applyHandling(
                hub.context(),
                HubHandlingInput.ResolveDiscrepancy(
                    page.stockRevision,
                    page.catalogRevision,
                    issue.id,
                    issue.revision,
                    "Inspected packaging; original observation retained",
                ),
            )
            assertEquals(0U, runtime.discrepancies(HubHandlingQuery()).total)
            val closed = runtime.discrepancies(HubHandlingQuery(openOnly = false)).rows.single()
            assertEquals(hub.owner, closed.resolvedBy)
            assertEquals(issue.observation, closed.value.observation)
            val original = runtime.handlingOperation(opened.operationId)
            assertEquals(opened, original)
            assertNull(assertIs<HubHandlingResult.DiscrepancyChanged>(original?.result).value.resolution)
            assertEquals(before, runtime.stock(HubStockQuery()).rows)
            assertEquals(work, runtime.work(HubHandlingQuery()).rows)
            assertEquals(hub.receipt, runtime.receiptOperation(hub.receipt.operationId))
        }

    @Test
    fun explicitPortionsRetainOriginalLineageAndWorkAcrossNativeRestart() =
        withHub { hub ->
            val runtime = hub.runtime
            val original = runtime.group(hub.receipt.initialPosition.groupId)
            val before =
                runtime
                    .work(HubHandlingQuery())
                    .rows
                    .single()
                    .work
            val request =
                HubHandlingInput.Split(
                    original.stockRevision,
                    original.catalogRevision,
                    original.group.id,
                    original.group.revision,
                    8U,
                    HubPhysicalIdentification("Blue eight", "Eight separately tagged crates"),
                    HubOccurrence(HubOccurrenceKind.Unknown),
                    "Observed separate stack at Receiving",
                )
            assertFailsWith<ApplicationFailure> {
                runtime.applyHandling(hub.context(), request.copy(identification = HubPhysicalIdentification("", "")))
            }
            assertEquals(original, runtime.group(original.group.id))
            val context = hub.context()
            val separated = runtime.applyHandling(context, request)
            val result = assertIs<HubHandlingResult.Separated>(separated.result)
            assertEquals(hub.receiving, result.location.id)
            assertEquals(12U, result.remainingPacks)
            val child = runtime.group(result.portionGroupId)
            runtime.applyHandling(
                hub.context(),
                request.copy(
                    expectedStockRevision = child.stockRevision,
                    expectedCatalogRevision = child.catalogRevision,
                    groupId = child.group.id,
                    expectedGroupRevision = child.group.revision,
                    wholePacks = 5U,
                    identification = HubPhysicalIdentification("Green five", "Five separately tagged crates"),
                ),
            )
            val query = HubLineageQuery(hub.receipt.trace.id)
            val lineage = runtime.lineage(query)
            assertEquals(3U, lineage.total)
            assertEquals(20UL, lineage.rows.sumOf { it.wholePacks.toULong() })
            assertEquals(listOf(3U, 5U, 12U), lineage.rows.map { it.wholePacks }.sorted())
            assertEquals(2, lineage.rows.count { it.createdKind == HubGroupCreationKind.Separation })
            assertEquals(
                before,
                runtime
                    .work(HubHandlingQuery())
                    .rows
                    .single()
                    .work,
            )
            assertEquals(separated, runtime.applyHandling(context, request))
            assertFailsWith<ApplicationFailure> {
                runtime.applyHandling(
                    hub.context(),
                    request.copy(
                        expectedStockRevision = lineage.stockRevision,
                        identification = HubPhysicalIdentification("Other eight", "Another tag"),
                    ),
                )
            }
            runtime.restart()
            runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
            assertEquals(lineage, runtime.lineage(query))
            assertEquals(separated, runtime.handlingOperation(separated.operationId))
            assertEquals(hub.receipt, runtime.receiptOperation(hub.receipt.operationId))
            runtime.execute(ApplicationCommand.SignOut)
            assertFailsWith<ApplicationFailure> { runtime.lineage(query) }
        }

    private fun withHub(block: suspend (HandlingFixture) -> Unit) =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-handling-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            try {
                block(HandlingFixture.open(runtime))
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }
}

private class HandlingFixture(
    val runtime: TestBridgeHarvestCircleRuntime,
    val owner: IdentityId,
    val receiving: String,
    val a3: String,
    val b3: String,
    val receipt: HubStockReceipt,
) {
    fun context() =
        RequestContext(OperationId.from(org.harvestcircle.ffi.generateOperationIdV7()), runtime.currentSnapshot().revision, 10_000UL)

    fun move(
        group: HubGroupView,
        target: String,
        packs: UInt,
    ) = HubHandlingInput.Move(
        group.stockRevision,
        group.catalogRevision,
        group.group.id,
        group.group.revision,
        target,
        packs,
        if (packs < group.group.wholePacks) HubPhysicalIdentification("Blue stack", "Eight tagged crates in a separate stack") else null,
        HubOccurrence(HubOccurrenceKind.Unknown),
        "Manually confirmed actual arrival",
    )

    companion object {
        suspend fun open(runtime: TestBridgeHarvestCircleRuntime): HandlingFixture {
            fun context() =
                RequestContext(
                    OperationId.from(org.harvestcircle.ffi.generateOperationIdV7()),
                    runtime.currentSnapshot().revision,
                    10_000UL,
                )
            runtime.bootstrap()
            val generated = runtime.prepareLocalIdentity()
            runtime.execute(ApplicationCommand.AcknowledgeGeneratedIdentity(generated.requestId, context()))
            generated.backup.clear()
            runtime.execute(ApplicationCommand.ActivateIdentity(generated.identity.id))
            runtime.initializeHub(context())

            suspend fun add(value: HubReferenceValue): String {
                val catalog = runtime.references(HubReferenceQuery())
                return runtime.editReference(context(), HubReferenceEdit(catalog.catalogRevision, value = value)).current.id
            }
            val product = add(HubReferenceValue(HubReferenceKind.Product, "Fictional apples"))
            val pack = add(HubReferenceValue(HubReferenceKind.Pack, "Original crate", productId = product))

            suspend fun location(
                label: String,
                purpose: HubLocationPurpose,
            ) = add(HubReferenceValue(HubReferenceKind.Location, label, locationPurpose = purpose))
            val receiving = location("Receiving", HubLocationPurpose.Receiving)
            val a3 = location("A3", HubLocationPurpose.Storage)
            val b3 = location("B3", HubLocationPurpose.Storage)
            val page = runtime.stock(HubStockQuery())
            val receipt =
                runtime.receiveStock(
                    context(),
                    HubReceiptInput(
                        page.stockRevision,
                        page.catalogRevision,
                        pack,
                        null,
                        null,
                        HubSourceInput(),
                        20U,
                        receiving,
                        a3,
                        HubOccurrence(HubOccurrenceKind.Unknown),
                        "Manually counted twenty fictional crates",
                    ),
                )
            return HandlingFixture(runtime, generated.identity.id, receiving, a3, b3, receipt)
        }
    }
}
