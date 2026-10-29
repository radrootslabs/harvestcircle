package org.harvestcircle.integration

import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationCommand
import org.harvestcircle.application.ApplicationFailure
import org.harvestcircle.application.HubAcceptanceStatus
import org.harvestcircle.application.HubCountBlockReason
import org.harvestcircle.application.HubCountDifference
import org.harvestcircle.application.HubCountObserveInput
import org.harvestcircle.application.HubCountPostInput
import org.harvestcircle.application.HubCountReview
import org.harvestcircle.application.HubLocalOutputRequest
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubOccurrenceKind
import org.harvestcircle.application.HubOpeningInput
import org.harvestcircle.application.HubPhysicalIdentification
import org.harvestcircle.application.HubPopulationOrigin
import org.harvestcircle.application.HubSourceCertainty
import org.harvestcircle.application.HubSourceInput
import org.harvestcircle.application.HubStockQuery
import org.harvestcircle.application.HubTraceHistoryKind
import org.harvestcircle.application.HubTraceHistoryQuery
import org.harvestcircle.application.HubTraceSnapshotQuery
import org.harvestcircle.application.HubWorkProgress
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class CountNativeAcceptanceTest {
    @Test
    fun generatedCountBoundaryPreservesObservationOriginalsWorkAndShortageTraceAcrossRestart(): Unit =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-count-native-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            try {
                val hub = HandlingFixture.open(runtime)
                val group = runtime.group(hub.receipt.initialPosition.groupId)
                val countContext = hub.context()
                val observed =
                    runtime.observeCount(
                        countContext,
                        HubCountObserveInput(
                            group.stockRevision,
                            group.catalogRevision,
                            group.group.id,
                            group.group.revision,
                            18U,
                            counted(),
                            "Fictional observed eighteen whole packs",
                        ),
                    )
                assertEquals(20U, observed.recordedWholePacks)
                assertEquals(18U, observed.countedWholePacks)
                assertEquals(countContext.operationId, observed.operationId)
                val before = runtime.stock(HubStockQuery())
                assertEquals(
                    20U,
                    before.rows
                        .single()
                        .position.wholePacks,
                )
                assertEquals(
                    20U,
                    before.rows
                        .single()
                        .putAway
                        ?.remaining,
                )
                val review = runtime.countReview(observed.id)
                assertTrue(review.ready)
                assertEquals(HubCountDifference.Shortage(2U), review.difference)
                val input = posting(review)
                val postContext = hub.context()
                val posted = runtime.postCount(postContext, input)
                assertEquals(postContext.operationId, posted.operationId)
                assertEquals(observed, posted.observation)
                assertEquals(20U, posted.before.wholePacks)
                assertEquals(18U, posted.onHandAfter)
                val work = posted.workChanges.single()
                assertEquals(0U, work.after.quantity.completed)
                assertEquals(2U, work.after.quantity.noLongerRequired)
                assertEquals(18U, work.after.quantity.remaining)
                assertEquals(HubWorkProgress.NoLongerRequired, posted.removedWork.single().removedProgress)
                assertNull(posted.surplus)
                assertEquals(posted, runtime.postCount(postContext, input))
                assertEquals(hub.receipt, runtime.receiptOperation(hub.receipt.operationId))
                val trace = runtime.traceSnapshot(HubTraceSnapshotQuery(hub.receipt.trace.id))
                assertEquals(18U, trace.onHandWholePacks)
                assertEquals(0U, trace.departedWholePacks)
                assertEquals(2U, trace.countedLossWholePacks)
                assertTrue(trace.exits.isEmpty())
                assertEquals(posted.id, trace.countLosses.single().reconciliationId)
                assertEquals(posted.confirmedOnHandAt, trace.countLosses.single().confirmedOnHandAt)
                val html =
                    runtime
                        .localOutput(
                            HubLocalOutputRequest.Trace(hub.receipt.trace.id, trace.stockRevision, trace.catalogRevision),
                        ).html
                assertTrue(html.contains("Unexplained count shortage"))
                assertTrue(html.contains("loss time and cause unknown"))
                assertTrue(html.contains("&lt;count&gt;"))
                assertTrue(
                    runtime.traceHistory(HubTraceHistoryQuery(hub.receipt.trace.id)).rows.any {
                        it.kind ==
                            HubTraceHistoryKind.CountReconciled
                    },
                )
                runtime.restart()
                runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                assertEquals(observed, runtime.countObservationOperation(countContext.operationId))
                assertEquals(posted, runtime.countPostOperation(postContext.operationId))
                assertEquals(trace, runtime.traceSnapshot(HubTraceSnapshotQuery(hub.receipt.trace.id)))
                runtime.execute(ApplicationCommand.SignOut)
                assertFailsWith<ApplicationFailure> { runtime.countObservationOperation(countContext.operationId) }
                assertFailsWith<ApplicationFailure> { runtime.countPostOperation(postContext.operationId) }
                assertFailsWith<ApplicationFailure> { runtime.countReview(observed.id) }
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }

    @Test
    fun generatedOpeningAndPositiveCountKeepSeparateUnknownPopulationsAndOriginalAcceptance(): Unit =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-count-opening-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            try {
                val hub = HandlingFixture.open(runtime)
                val stock = runtime.stock(HubStockQuery())
                val context = hub.context()
                val input =
                    HubOpeningInput(
                        stock.stockRevision,
                        stock.catalogRevision,
                        hub.receipt.trace.variant.id,
                        hub.a3,
                        8U,
                        HubPhysicalIdentification("Opening eight", "Eight separately counted fictional packs"),
                        HubSourceInput(),
                        null,
                        counted(),
                        "Earlier intake and handling are unknown",
                    )
                val opened = runtime.recordOpening(context, input)
                assertEquals(opened, runtime.recordOpening(context, input))
                assertEquals(HubPopulationOrigin.CountedOpening(null), opened.original.population.origin)
                assertEquals(HubSourceCertainty.Unknown, opened.original.population.source.certainty)
                assertEquals(HubAcceptanceStatus.ReviewRequired, opened.original.acceptance)
                assertTrue(
                    opened.original.group.work
                        .isEmpty(),
                )
                val group = runtime.group(opened.original.group.id)
                val observed =
                    runtime.observeCount(
                        hub.context(),
                        HubCountObserveInput(
                            group.stockRevision,
                            group.catalogRevision,
                            group.group.id,
                            group.group.revision,
                            10U,
                            counted(),
                            "Counted two extra packs of unestablished origin",
                        ),
                    )
                val review = runtime.countReview(observed.id)
                assertEquals(HubCountDifference.Surplus(2U), review.difference)
                val postContext = hub.context()
                val posted =
                    runtime.postCount(
                        postContext,
                        posting(
                            review,
                        ).copy(
                            surplusIdentification =
                                HubPhysicalIdentification(
                                    "Separate counted extras",
                                    "Two independently identified fictional packs",
                                ),
                        ),
                    )
                assertEquals(8U, posted.onHandAfter)
                val surplus = assertNotNull(posted.surplus)
                assertEquals(2U, surplus.population.originalWholePacks)
                assertEquals(HubSourceCertainty.Unknown, surplus.population.source.certainty)
                assertEquals(HubAcceptanceStatus.ReviewRequired, surplus.acceptance)
                assertTrue(surplus.group.work.isEmpty())
                assertEquals(HubPopulationOrigin.CountedSurplus(observed.id, opened.original.population.id), surplus.population.origin)
                assertIs<HubCountDifference.Surplus>(posted.difference)
                assertEquals(opened, runtime.openingOperation(context.operationId))
                assertEquals(hub.receipt, runtime.receiptOperation(hub.receipt.operationId))
                val current = runtime.stock(HubStockQuery()).rows
                assertEquals(8U, current.single { it.trace.id == opened.original.population.id }.position.wholePacks)
                assertEquals(2U, current.single { it.trace.id == surplus.population.id }.position.wholePacks)
                assertNull(current.single { it.trace.id == surplus.population.id }.putAway)
                runtime.restart()
                runtime.execute(ApplicationCommand.ActivateIdentity(hub.owner))
                assertEquals(opened, runtime.openingOperation(context.operationId))
                assertEquals(posted, runtime.countPostOperation(postContext.operationId))
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }

    @Test
    fun generatedReviewBlocksMovedScopeWithoutPostingAnUnjustifiedShortage(): Unit =
        runBlocking {
            val root = Files.createTempDirectory("harvestcircle-count-stale-")
            val runtime = TestBridgeHarvestCircleRuntime.openDemo(root.toString())
            try {
                val hub = HandlingFixture.open(runtime)
                val group = runtime.group(hub.receipt.initialPosition.groupId)
                val context = hub.context()
                val observed =
                    runtime.observeCount(
                        context,
                        HubCountObserveInput(
                            group.stockRevision,
                            group.catalogRevision,
                            group.group.id,
                            group.group.revision,
                            18U,
                            counted(),
                            "Counted before a later movement",
                        ),
                    )
                val moving = runtime.group(group.group.id)
                runtime.applyHandling(hub.context(), hub.move(moving, hub.a3, 20U))
                val before = runtime.stock(HubStockQuery())
                val review = runtime.countReview(observed.id)
                assertFalse(review.ready)
                assertTrue(HubCountBlockReason.ScopeChanged in review.blockers)
                val postContext = hub.context()
                assertFailsWith<ApplicationFailure> { runtime.postCount(postContext, posting(review)) }
                assertNull(runtime.countPostOperation(postContext.operationId))
                assertEquals(before, runtime.stock(HubStockQuery()))
                assertEquals(observed, runtime.countObservationOperation(context.operationId))
            } finally {
                runtime.close()
                Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach(Files::deleteIfExists) }
            }
        }

    private fun counted() = HubOccurrence(HubOccurrenceKind.Known, 1_700_000_000L)

    private fun posting(review: HubCountReview) =
        HubCountPostInput(
            review.stockRevision,
            review.catalogRevision,
            review.observation.id,
            true,
            null,
            "Reviewed <count> and current native activity",
        )
}
