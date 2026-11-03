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
class ActualPresenterTest {
    @Test
    fun unknownReportCanBeSavedWhenStockAndContextAreUnavailable() =
        runTest {
            val runtime = ActualFake()
            val presenter = ActualPresenter(runtime, null, null, null, null, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub", binding(1UL))
            presenter.beginReport()
            presenter.edit { it.copy(observation = "Reported departure; source, recipient and quantity not established") }
            presenter.refreshContext()
            runCurrent()
            assertNull(presenter.state.value.currentContext)
            assertFalse(presenter.state.value.needsReview)
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(1UL))
            runCurrent()
            val saved = assertIs<ActualSubmission.Recorded>(presenter.state.value.submission)
            val report = assertIs<ActualResult.Reported>(saved.result).value
            assertNull(report.reported.wholePacks)
            assertNull(report.reported.recipientId)
            assertTrue(report.reported.sourceClaims.isEmpty())
            assertEquals(1, runtime.writes)
            presenter.finishRecorded()
            runCurrent()
            assertEquals(HubActualReportFilter.All, presenter.state.value.filter)
            assertEquals(
                1,
                presenter.state.value.page
                    ?.rows
                    ?.size,
            )
            presenter.close()
        }

    @Test
    fun lostReplyAndAbsentOriginalFreezeTheOperationAndAllInputs() =
        runTest {
            val runtime =
                ActualFake().apply {
                    lostReply = true
                    hideOriginal = true
                }
            var allocated = 0
            val presenter =
                ActualPresenter(
                    runtime,
                    null,
                    null,
                    null,
                    null,
                    this,
                    OperationIdSource {
                        allocated++
                        StockFixtures.operation
                    },
                )
            presenter.attach("hub", binding(1UL))
            presenter.beginReport()
            presenter.edit { it.copy(observation = "Unverified physical departure", quantity = "2") }
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(1UL))
            presenter.submit(SnapshotRevision(1UL))
            runCurrent()
            val attempt = assertIs<ActualSubmission.Uncertain>(presenter.state.value.submission).attempt
            presenter.edit { it.copy(quantity = "3") }
            presenter.discard()
            presenter.beginReport()
            presenter.reviewRejected()
            presenter.checkOriginal()
            runCurrent()
            assertEquals(attempt, assertIs<ActualSubmission.Uncertain>(presenter.state.value.submission).attempt)
            assertEquals(
                "2",
                presenter.state.value.draft
                    ?.fields
                    ?.quantity,
            )
            presenter.submit(SnapshotRevision(1UL))
            assertEquals(1, allocated)
            assertEquals(1, runtime.writes)
            runtime.hideOriginal = false
            presenter.checkOriginal()
            runCurrent()
            assertEquals(attempt, assertIs<ActualSubmission.Recorded>(presenter.state.value.submission).attempt)
            assertEquals(1, runtime.writes)
            presenter.close()
        }

    @Test
    fun sameIdentityNewNativeGenerationClearsDraftAndDiscardsLateReply() =
        runTest {
            val runtime = ActualFake().apply { barrier = CompletableDeferred() }
            val presenter = ActualPresenter(runtime, null, null, null, null, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub", binding(1UL))
            presenter.beginReport()
            presenter.edit { it.copy(observation = "Original owner's report") }
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(1UL))
            runCurrent()
            assertIs<ActualSubmission.Pending>(presenter.state.value.submission)
            presenter.attach("hub", binding(2UL))
            assertFalse(presenter.state.value.available)
            assertNull(presenter.state.value.draft)
            requireNotNull(runtime.barrier).complete(Unit)
            runCurrent()
            assertNull(presenter.state.value.submission)
            assertEquals(1, runtime.writes)
            presenter.beginReport()
            assertNull(presenter.state.value.draft)
        }

    @Test
    fun parkingPreservesUnsubmittedRawClaimsButClearsConfirmation() =
        runTest {
            val runtime = ActualFake()
            val presenter = ActualPresenter(runtime, null, null, null, null, this, OperationIdSource { error("No operation") })
            presenter.attach("hub", binding(1UL))
            presenter.beginReport()
            presenter.edit {
                it.copy(
                    observation = "Reported future clock",
                    occurrenceKind = HubOccurrenceKind.Known,
                    occurrenceStart = "2099-01-01T00:00:00Z",
                )
            }
            presenter.confirm(true)
            assertTrue(requireNotNull(presenter.state.value.draft).fields.confirmed)
            presenter.park()
            assertFalse(presenter.state.value.open)
            presenter.resume()
            val fields = requireNotNull(presenter.state.value.draft).fields
            assertEquals("2099-01-01T00:00:00Z", fields.occurrenceStart)
            assertFalse(fields.confirmed)
            presenter.edit { it.copy(observation = "x".repeat(1025)) }
            assertEquals(
                "Reported future clock",
                presenter.state.value.draft
                    ?.fields
                    ?.observation,
            )
            assertEquals(0, runtime.writes)
            presenter.close()
        }

    private fun binding(generation: ULong) = NativeSessionBinding("01".repeat(16), generation)
}

internal class ActualFake : HarvestCircleActualRuntime {
    var writes = 0
    var lostReply = false
    var hideOriginal = false
    var barrier: CompletableDeferred<Unit>? = null
    private var saved: HubActualReport? = null

    override suspend fun reportActual(
        context: RequestContext,
        input: HubActualReportInput,
    ): HubActualReport {
        withContext(NonCancellable) { barrier?.await() }
        writes++
        val report =
            HubActualReport(
                "09".repeat(16),
                context.operationId,
                "hub",
                2UL,
                5UL,
                StockFixtures.actor,
                UnixSeconds(100L),
                HubActualClockAssessment.Unknown,
                input,
            )
        saved = report
        if (lostReply) error("Reply lost after native commit")
        return report
    }

    override suspend fun actualReports(query: HubActualReportQuery) =
        HubActualReportPage(
            2UL,
            5UL,
            0U,
            if (saved ==
                null
            ) {
                0U
            } else {
                1U
            },
            false,
            listOfNotNull(saved?.let { HubActualReportRow(it, null) }),
        )

    override suspend fun actualReportOperation(operation: OperationId) = saved?.takeUnless { hideOriginal }

    override suspend fun actualReviewOperation(operation: OperationId): HubActualReviewReceipt? = null

    override suspend fun actualDeparturePreview(
        operation: OperationId,
        input: HubActualDepartureInput,
    ): HubActualDeparturePreview = error("No physical preview")

    override suspend fun actualLinkPreview(
        operation: OperationId,
        input: HubActualLinkInput,
    ): HubActualLinkPreview = error("No duplicate preview")

    override suspend fun reconcileActual(
        context: RequestContext,
        input: HubActualDepartureInput,
    ): HubActualReviewReceipt = error("No physical write")

    override suspend fun linkActual(
        context: RequestContext,
        input: HubActualLinkInput,
    ): HubActualReviewReceipt = error("No duplicate write")
}
