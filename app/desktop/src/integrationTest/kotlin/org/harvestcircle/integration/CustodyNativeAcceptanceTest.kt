package org.harvestcircle.integration

import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.ApplicationFailure
import org.harvestcircle.application.HubAcceptanceStatus
import org.harvestcircle.application.HubDepartureInput
import org.harvestcircle.application.HubDispatchActionInput
import org.harvestcircle.application.HubDispatchInput
import org.harvestcircle.application.HubDispatchLineInput
import org.harvestcircle.application.HubDispatchMaterialInput
import org.harvestcircle.application.HubDispatchReceipt
import org.harvestcircle.application.HubDispositionInput
import org.harvestcircle.application.HubDispositionKind
import org.harvestcircle.application.HubHandlingInput
import org.harvestcircle.application.HubHandlingQuery
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubPhysicalIdentification
import org.harvestcircle.application.HubPopulationOrigin
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.HubReturnClaim
import org.harvestcircle.application.HubReturnInput
import org.harvestcircle.application.HubReturnResolution
import org.harvestcircle.application.HubReturnTiming
import org.harvestcircle.application.HubSourceCertainty
import org.harvestcircle.application.HubSourceInput
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubTraceQuery
import org.harvestcircle.application.HubTraceScope
import org.harvestcircle.application.HubTraceSelection
import org.harvestcircle.application.HubTraceSnapshotQuery
import org.harvestcircle.application.HubTreatmentScope
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class CustodyNativeAcceptanceTest {
    @Test
    fun actualReturnedTwoKeepOriginalSixDepartureAndFourteenEligibleThenDistinctDispositionAcrossRestart(): Unit =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-return-disposition-native-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            try {
                val hub = HandlingFixture.open(runtime)
                val originalGroup = hub.receipt.initialPosition.groupId
                val current = runtime.group(originalGroup)
                runtime.applyHandling(
                    hub.context(),
                    HubHandlingInput.SetAcceptance(
                        current.stockRevision,
                        current.catalogRevision,
                        listOf(originalGroup),
                        HubAcceptanceStatus.Accepted,
                        HubOccurrence(HubOccurrenceKind.Unknown),
                        "Original twenty accepted independently of later returns",
                    ),
                )
                val six = hub.split(originalGroup, 6U, "Actual six for original recipient")
                val paper = custodyIssue(hub, listOf(six), "Fictional original recipient")
                val departed = runtime.applyDeparture(hub.context(), custodyDepartureInput(runtime, paper))
                val originalWork = runtime.work(HubHandlingQuery(openOnly = false))
                val stock = runtime.stock(HubStockQuery())
                val reference = runtime.references(HubReferenceQuery())
                val returnedBy =
                    runtime
                        .editReference(
                            hub.context(),
                            HubReferenceEdit(
                                reference.catalogRevision,
                                value = HubReferenceValue(HubReferenceKind.Party, "Independent returning party"),
                            ),
                        ).current
                val input =
                    HubReturnInput(
                        stock.stockRevision,
                        runtime.references(HubReferenceQuery()).catalogRevision,
                        hub.receipt.trace.variant.id,
                        hub.receiving,
                        returnedBy.id,
                        HubSourceInput(),
                        2U,
                        HubPhysicalIdentification(
                            "Returned two separate crates",
                            "Observed tagged returned crates, separate from original fourteen",
                        ),
                        null,
                        HubReturnClaim(departed.id, six),
                        HubOccurrence(HubOccurrenceKind.Unknown),
                        "Actually returned, source and occurrence unknown",
                    )
                val context = hub.context()
                val returned = runtime.applyReturn(context, input)
                assertEquals(HubReturnResolution.EstablishedScope, returned.resolution)
                assertEquals(HubReturnTiming.Uncertain, returned.timing)
                assertEquals(returnedBy, returned.returnedBy)
                assertEquals(
                    paper.record.versions
                        .last()
                        .material.recipient,
                    assertNotNull(returned.departure).recipient,
                )
                assertEquals(HubPopulationOrigin.Returned(returned.id), returned.population.origin)
                assertEquals(HubSourceCertainty.Unknown, returned.population.source.certainty)
                assertEquals(HubAcceptanceStatus.ReviewRequired, returned.acceptance)
                assertTrue(returned.group.work.isEmpty())
                assertEquals(originalWork.rows, runtime.work(HubHandlingQuery(openOnly = false)).rows)
                val after = runtime.stock(HubStockQuery())
                assertEquals(16UL, after.rows.sumOf { it.position.wholePacks.toULong() })
                val oldTreatment = runtime.treatment(HubTreatmentScope(hub.receipt.trace.id))
                val newTreatment = runtime.treatment(HubTreatmentScope(returned.population.id))
                assertEquals(14U, oldTreatment.summary.departureEligible)
                assertEquals(2U, newTreatment.summary.reviewRequired)
                assertEquals(0U, newTreatment.summary.departureEligible)
                assertEquals(departed, runtime.departureOperation(departed.operationId))
                assertEquals(returned, runtime.applyReturn(hub.context().copy(operationId = context.operationId), input))
                assertEquals(after, runtime.stock(HubStockQuery()))
                val group = runtime.group(returned.group.id)
                val removal =
                    HubDispositionInput(
                        group.stockRevision,
                        group.catalogRevision,
                        group.group.id,
                        group.group.revision,
                        2U,
                        HubDispositionKind.Destroyed,
                        HubOccurrence(HubOccurrenceKind.Unknown),
                        "Observed complete returned crates destroyed",
                    )
                val removalContext = hub.context()
                val disposed = runtime.applyDisposition(removalContext, removal)
                assertEquals(HubDispositionKind.Destroyed, disposed.kind)
                assertEquals(2U, disposed.before.wholePacks)
                assertEquals(hub.receipt.trace.product, disposed.product)
                assertEquals(hub.receipt.trace.variant, disposed.variant)
                assertEquals(disposed, runtime.applyDisposition(hub.context().copy(operationId = removalContext.operationId), removal))
                val trace = runtime.traceSnapshot(HubTraceSnapshotQuery(returned.population.id))
                assertEquals(0U, trace.onHandWholePacks)
                assertEquals(0U, trace.departedWholePacks)
                assertEquals(0U, trace.countedLossWholePacks)
                assertEquals(2U, trace.disposedWholePacks)
                assertEquals(1, trace.dispositions.size)
                assertEquals(disposed.id, trace.dispositions.single().id)
                val page = runtime.tracePage(HubTraceQuery(HubTraceSelection(HubTraceScope.Receipt, receiptId = returned.population.id)))
                assertEquals(2U, page.rows.single().disposedWholePacks)
                assertEquals(14UL, runtime.stock(HubStockQuery()).rows.sumOf { it.position.wholePacks.toULong() })
                val renameCatalog = runtime.references(HubReferenceQuery())
                runtime.editReference(
                    hub.context(),
                    HubReferenceEdit(
                        renameCatalog.catalogRevision,
                        disposed.product.id,
                        disposed.product.revision,
                        disposed.product.value.copy(label = "Renamed product after disposition"),
                    ),
                )
                assertEquals(disposed, runtime.dispositionOperation(disposed.operationId))
                runtime.restart()
                runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                assertEquals(returned, runtime.returnOperation(returned.operationId))
                assertEquals(disposed, runtime.dispositionOperation(disposed.operationId))
                assertEquals(departed, runtime.departureOperation(departed.operationId))
                runtime.execute(ApplicationCommand.SignOut)
                assertFailsWith<ApplicationFailure> { runtime.returnOperation(returned.operationId) }
                assertFailsWith<ApplicationFailure> { runtime.dispositionOperation(disposed.operationId) }
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }
}

private suspend fun custodyIssue(
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

private suspend fun custodyDepartureInput(
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
