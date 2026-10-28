package org.harvestcircle.application

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class HandlingPresenterTest {
    @Test
    fun splitRetainsObservedInputAcrossNavigationAndFreezesUncertainSubmission() =
        runTest {
            val runtime = HandlingFake()
            var ids = 0
            val presenter =
                HandlingPresenter(
                    runtime,
                    runtime,
                    this,
                    OperationIdSource {
                        ids++
                        StockFixtures.operation
                    },
                )
            presenter.attach("hub")
            runCurrent()
            presenter.inspectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.begin(HandlingAction.Split)
            presenter.edit(HandlingFixtures.fields.copy(targetId = null))
            presenter.parkDraft()
            presenter.chooseTab(HandlingTab.Discrepancies)
            runCurrent()
            presenter.begin(HandlingAction.Move)
            assertIs<HandlingDraft.Split>(presenter.state.value.draft)
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            val frozen = assertIs<HandlingSubmission.Uncertain>(presenter.state.value.submission)
            val input = assertIs<HubHandlingInput.Split>(frozen.attempt.input)
            assertEquals(8U, input.wholePacks)
            assertEquals(HubPhysicalIdentification("Blue stack", "Eight separately tagged crates"), input.identification)
            assertEquals(HubOccurrence(HubOccurrenceKind.Unknown), input.occurrence)
            presenter.edit(HandlingFixtures.fields.copy(quantity = "99"))
            presenter.submit(SnapshotRevision(8UL))
            runCurrent()
            assertEquals(1, ids)
            assertEquals(1, runtime.writes.size)
            assertEquals(
                "8",
                presenter.state.value.draft
                    ?.fields
                    ?.quantity,
            )
            presenter.close()
        }

    @Test
    fun lineageContinuationRetainsTraceAndBothNativeVersions() =
        runTest {
            val queries = mutableListOf<HubLineageQuery>()
            val runtime =
                object : HandlingFake() {
                    override suspend fun lineage(query: HubLineageQuery): HubLineagePage {
                        queries += query
                        return super.lineage(query).copy(total = 2U, offset = query.offset, more = query.offset == 0U)
                    }
                }
            val presenter = HandlingPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.inspectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.nextLineagePage()
            runCurrent()
            val query = queries.last()
            assertEquals(StockFixtures.trace.id, query.traceId)
            assertEquals(2UL, query.expectedStockRevision)
            assertEquals(5UL, query.expectedCatalogRevision)
            assertEquals(1U, query.offset)
            assertEquals(
                1U,
                presenter.state.value.lineage
                    ?.offset,
            )
            presenter.close()
        }

    @Test
    fun lateLineageReadCannotRestoreRecordsAfterHubReplacement() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : HandlingFake() {
                    override suspend fun lineage(query: HubLineageQuery): HubLineagePage =
                        withContext(NonCancellable) {
                            gate.await()
                            super.lineage(query)
                        }
                }
            val presenter = HandlingPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("old")
            runCurrent()
            presenter.inspectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.attach("new")
            runCurrent()
            gate.complete(Unit)
            runCurrent()
            assertNull(presenter.state.value.group)
            assertNull(presenter.state.value.lineage)
            presenter.close()
        }

    @Test
    fun uncertainReplyFreezesInputAndOnlyLooksUpTheSameOperation() =
        runTest {
            val runtime = HandlingFake()
            var ids = 0
            val presenter =
                HandlingPresenter(
                    runtime,
                    runtime,
                    this,
                    OperationIdSource {
                        ids += 1
                        StockFixtures.operation
                    },
                )
            presenter.attach("hub")
            runCurrent()
            presenter.inspectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.begin(HandlingAction.Move)
            presenter.edit(HandlingFixtures.fields)
            presenter.submit(SnapshotRevision(7UL))
            presenter.edit(HandlingFixtures.fields.copy(quantity = "99"))
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            val unknown = assertIs<HandlingSubmission.Uncertain>(presenter.state.value.submission)
            assertEquals(8U, assertIs<HubHandlingInput.Move>(unknown.attempt.input).wholePacks)
            assertEquals(
                "8",
                presenter.state.value.draft
                    ?.fields
                    ?.quantity,
            )
            val original = runtime.stored
            runtime.stored = null
            presenter.checkOriginal()
            runCurrent()
            presenter.reviewRejected()
            presenter.discardDraft()
            presenter.submit(SnapshotRevision(8UL))
            runCurrent()
            assertIs<HandlingSubmission.Uncertain>(presenter.state.value.submission)
            assertEquals(1, ids)
            assertEquals(1, runtime.writes.size)
            runtime.stored = original
            presenter.checkOriginal()
            runCurrent()
            assertIs<HandlingSubmission.Recorded>(presenter.state.value.submission)
            assertEquals(listOf(StockFixtures.operation, StockFixtures.operation), runtime.lookups)
            presenter.finishRecorded()
            runCurrent()
            assertNull(presenter.state.value.draft)
            presenter.close()
        }

    @Test
    fun staleInputRequiresExplicitScopeReviewAndKeepsPhysicalObservations() =
        runTest {
            val conflict =
                ApplicationFailure(
                    ApplicationProblem(
                        ApplicationErrorCode.HubConflict,
                        ApplicationErrorCategory.Conflict,
                        false,
                        RecoveryAction.None,
                        StockFixtures.operation,
                        "Stock context changed",
                    ),
                )
            val runtime = HandlingFake().apply { failure = conflict }
            val presenter = HandlingPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.inspectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.begin(HandlingAction.Move)
            presenter.edit(HandlingFixtures.fields)
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            presenter.checkOriginal()
            runCurrent()
            assertIs<HandlingSubmission.Rejected>(presenter.state.value.submission)
            presenter.reviewRejected()
            assertTrue(presenter.state.value.needsReview)
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            assertEquals(1, runtime.writes.size)
            runtime.groupView = runtime.groupView.copy(stockRevision = 4UL, group = runtime.groupView.group.copy(revision = 3UL))
            presenter.reviewContext()
            runCurrent()
            assertEquals(
                HandlingFixtures.fields,
                presenter.state.value.review
                    ?.draft
                    ?.fields,
            )
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            assertEquals(1, runtime.writes.size)
            presenter.acceptReview()
            assertFalse(presenter.state.value.needsReview)
            val reviewed = assertIs<HandlingDraft.Move>(presenter.state.value.draft)
            assertEquals(4UL, reviewed.versions.stock)
            assertEquals(3UL, reviewed.selected.group.revision)
            assertEquals(HandlingFixtures.fields, reviewed.fields)
            presenter.close()
        }

    @Test
    fun navigationAndAnotherActionRetainTheOriginalUnsubmittedDraft() =
        runTest {
            val runtime = HandlingFake()
            val presenter = HandlingPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.inspectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.begin(HandlingAction.Move)
            presenter.edit(HandlingFixtures.fields)
            presenter.parkDraft()
            presenter.chooseTab(HandlingTab.Discrepancies)
            runCurrent()
            presenter.begin(HandlingAction.ResolveDiscrepancy)
            assertIs<HandlingDraft.Move>(presenter.state.value.draft)
            assertEquals(
                HandlingFixtures.fields,
                presenter.state.value.draft
                    ?.fields,
            )
            assertTrue(presenter.state.value.formOpen)
            presenter.close()
            assertEquals(HandlingViewState(), presenter.state.value)
        }

    @Test
    fun nonCooperativeOldReadCannotReplaceTheNewHubState() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            var reads = 0
            val runtime =
                object : HandlingFake() {
                    override suspend fun work(query: HubHandlingQuery): HubWorkPage {
                        reads += 1
                        if (reads == 1) {
                            return withContext(NonCancellable) {
                                gate.await()
                                HandlingFixtures.page
                            }
                        }
                        return HandlingFixtures.page.copy(rows = emptyList(), total = 0U)
                    }
                }
            val presenter = HandlingPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("old")
            runCurrent()
            presenter.attach("new")
            runCurrent()
            assertEquals(
                0U,
                presenter.state.value.work
                    ?.total,
            )
            gate.complete(Unit)
            runCurrent()
            assertEquals(
                0U,
                presenter.state.value.work
                    ?.total,
            )
            presenter.close()
        }

    @Test
    fun continuationKeepsTheLoadedFilterAndBothNativeVersions() =
        runTest {
            val runtime = HandlingFake().apply { workPage = HandlingFixtures.page.copy(total = 2U, more = true) }
            val presenter = HandlingPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.search("Unsubmitted search text")
            presenter.nextPage()
            runCurrent()
            val continued = runtime.queries.last()
            assertEquals("", continued.text)
            assertEquals(1U, continued.offset)
            assertEquals(2UL, continued.expectedStockRevision)
            assertEquals(5UL, continued.expectedCatalogRevision)
            presenter.close()
        }

    @Test
    fun malformedCountDoesNotAllocateAnOperationOrEraseTheDraft() =
        runTest {
            val runtime = HandlingFake()
            var ids = 0
            val presenter =
                HandlingPresenter(
                    runtime,
                    runtime,
                    this,
                    OperationIdSource {
                        ids += 1
                        StockFixtures.operation
                    },
                )
            presenter.attach("hub")
            runCurrent()
            presenter.inspectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.begin(HandlingAction.Move)
            presenter.edit(HandlingFixtures.fields.copy(quantity = "-1"))
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            assertEquals(0, ids)
            assertTrue(runtime.writes.isEmpty())
            assertEquals(
                "-1",
                presenter.state.value.draft
                    ?.fields
                    ?.quantity,
            )
            presenter.close()
        }
}
