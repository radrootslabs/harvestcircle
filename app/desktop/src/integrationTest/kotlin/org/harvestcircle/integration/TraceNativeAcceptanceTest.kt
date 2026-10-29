package org.harvestcircle.integration

import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.ApplicationFailure
import org.harvestcircle.application.HubAcceptanceStatus
import org.harvestcircle.application.HubDispatchActionInput
import org.harvestcircle.application.HubDispatchInput
import org.harvestcircle.application.HubDispatchLineInput
import org.harvestcircle.application.HubDispatchMaterialInput
import org.harvestcircle.application.HubLocalOutputRequest
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubReceiptInput
import org.harvestcircle.application.HubReferenceEdit
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceQuery
import org.harvestcircle.application.HubReferenceValue
import org.harvestcircle.application.HubSourceCertainty
import org.harvestcircle.application.HubSourceInput
import org.harvestcircle.application.HubSourceKind
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubTraceHistoryKind
import org.harvestcircle.application.HubTraceHistoryQuery
import org.harvestcircle.application.HubTraceQuery
import org.harvestcircle.application.HubTraceScope
import org.harvestcircle.application.HubTraceSelection
import org.harvestcircle.application.HubTraceSnapshotQuery
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class TraceNativeAcceptanceTest {
    @Test
    fun nativeTracePaginationOriginalFactsAndOutputSurviveReferenceChangesAndRestart(): Unit =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-trace-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            try {
                val hub = HandlingFixture.open(runtime)
                val catalog = runtime.references(HubReferenceQuery())
                val issuer =
                    runtime
                        .editReference(
                            hub.context(),
                            HubReferenceEdit(
                                catalog.catalogRevision,
                                value = HubReferenceValue(HubReferenceKind.Party, "Fictional issuer <sample>"),
                            ),
                        ).current
                val stock = runtime.stock(HubStockQuery())
                val known =
                    runtime.receiveStock(
                        hub.context(),
                        HubReceiptInput(
                            stock.stockRevision,
                            stock.catalogRevision,
                            hub.receipt.trace.variant.id,
                            null,
                            null,
                            HubSourceInput(
                                issuerId = issuer.id,
                                kind = HubSourceKind.SupplierReference,
                                code = "FX-1027",
                                provenance = "Original manually observed label",
                                certainty = HubSourceCertainty.Established,
                            ),
                            20U,
                            hub.receiving,
                            hub.a3,
                            HubOccurrence(HubOccurrenceKind.Unknown),
                            "Another distinct twenty original crates",
                        ),
                    )
                assertNull(known.trace.producer)
                val sourceId = assertNotNull(known.trace.source.id)
                val selection = HubTraceSelection(HubTraceScope.SourceImpact, sourceId = sourceId)
                val first = runtime.tracePage(HubTraceQuery(selection, limit = 1U))
                assertEquals(2U, first.total)
                assertTrue(first.more)
                val continuation = HubTraceQuery(selection, first.stockRevision, first.catalogRevision, 1U, 1U)
                val second = runtime.tracePage(continuation)
                assertEquals(setOf(hub.receipt.trace.id, known.trace.id), (first.rows + second.rows).map { it.trace.id }.toSet())
                val group = runtime.group(known.initialPosition.groupId)
                val prepared =
                    runtime.applyDispatch(
                        hub.context(),
                        HubDispatchInput.Prepare(
                            HubDispatchMaterialInput(
                                group.stockRevision,
                                group.catalogRevision,
                                issuer.id,
                                listOf(HubDispatchLineInput(group.group.id, group.group.revision, 20U)),
                                "Sample material <script>never active</script>",
                            ),
                        ),
                    )
                val view = runtime.dispatchRecord(prepared.record.id)
                val issued =
                    runtime.applyDispatch(
                        hub.context(),
                        HubDispatchInput.Issue(
                            HubDispatchActionInput(
                                view.stockRevision,
                                view.catalogRevision,
                                prepared.record.id,
                                prepared.record.revision,
                                "Reviewed sample identification",
                            ),
                        ),
                    )
                val outputRequest =
                    HubLocalOutputRequest.Identification(
                        issued.record.id,
                        issued.record.versions
                            .single()
                            .id,
                    )
                val issuedOutput = runtime.localOutput(outputRequest)
                assertTrue(issuedOutput.html.contains("&lt;script&gt;never active&lt;/script&gt;"))
                assertTrue(issuedOutput.html.contains("Fictional issuer &lt;sample&gt;"))
                val snapshot = runtime.traceSnapshot(HubTraceSnapshotQuery(known.trace.id))
                assertEquals(20U, snapshot.onHandWholePacks)
                assertEquals(0U, snapshot.departedWholePacks)
                assertEquals(HubAcceptanceStatus.ReviewRequired, snapshot.onHand.single().acceptance)
                val traceOutputRequest = HubLocalOutputRequest.Trace(known.trace.id, snapshot.stockRevision, snapshot.catalogRevision)
                val traceOutput = runtime.localOutput(traceOutputRequest)
                assertEquals(traceOutput, runtime.localOutput(traceOutputRequest))
                val history = runtime.traceHistory(HubTraceHistoryQuery(known.trace.id))
                assertEquals(
                    listOf(
                        HubTraceHistoryKind.Receipt,
                        HubTraceHistoryKind.PrepareIdentification,
                        HubTraceHistoryKind.IssueIdentification,
                    ),
                    history.rows.map {
                        it.kind
                    },
                )
                val stockBeforeRename = runtime.stock(HubStockQuery()).rows
                val current = runtime.references(HubReferenceQuery())
                runtime.editReference(
                    hub.context(),
                    HubReferenceEdit(
                        current.catalogRevision,
                        issuer.id,
                        issuer.revision,
                        HubReferenceValue(HubReferenceKind.Party, "Current renamed issuer"),
                    ),
                )
                assertFailsWith<ApplicationFailure> { runtime.tracePage(continuation) }
                assertFailsWith<ApplicationFailure> { runtime.localOutput(traceOutputRequest) }
                assertEquals(issuedOutput, runtime.localOutput(outputRequest))
                assertEquals(stockBeforeRename, runtime.stock(HubStockQuery()).rows)
                val latest = runtime.traceSnapshot(HubTraceSnapshotQuery(known.trace.id))
                runtime.restart()
                runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                assertEquals(latest, runtime.traceSnapshot(HubTraceSnapshotQuery(known.trace.id)))
                assertEquals(issuedOutput, runtime.localOutput(outputRequest))
                runtime.execute(ApplicationCommand.SignOut)
                assertFailsWith<ApplicationFailure> { runtime.tracePage(HubTraceQuery()) }
                assertFailsWith<ApplicationFailure> { runtime.traceHistory(HubTraceHistoryQuery(known.trace.id)) }
                assertFailsWith<ApplicationFailure> { runtime.localOutput(outputRequest) }
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }
}
