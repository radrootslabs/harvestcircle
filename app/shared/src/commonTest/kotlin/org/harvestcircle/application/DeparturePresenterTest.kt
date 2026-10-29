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
class DeparturePresenterTest {
    @Test
    fun actualConfirmationIsRequiredAndEditingInvalidatesItWithoutRetargetingPaper() =
        runTest {
            val runtime = DepartureFake()
            val presenter = DeparturePresenter(runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            presenter.begin(runtime.view)
            runCurrent()
            presenter.edit(HandlingFields(observation = "All original crates actually left"))
            presenter.submit(SnapshotRevision(7UL))
            assertTrue(runtime.departureWrites.isEmpty())
            presenter.confirm(true)
            presenter.edit(HandlingFields(observation = "Actual departure witnessed with recipient"))
            assertFalse(requireNotNull(presenter.state.value.draft).confirmed)
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            val input = runtime.departureWrites.single().second
            assertEquals(
                runtime.request,
                HubDepartureReviewInput(input.preparationId, input.expectedPreparationRevision, input.identificationId),
            )
            assertEquals(
                7UL,
                runtime.departureWrites
                    .single()
                    .first.expectedRevision.value,
            )
            assertEquals("Actual departure witnessed with recipient", input.observation)
            assertIs<DepartureSubmission.Uncertain>(presenter.state.value.submission)
            presenter.close()
        }

    @Test
    fun typingDuringReviewAndNavigationRetainsInputButRequiresFreshExplicitReview() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : DepartureFake() {
                    override suspend fun departureReview(input: HubDepartureReviewInput): HubDepartureReview {
                        gate.await()
                        return super.departureReview(input)
                    }
                }
            val presenter = DeparturePresenter(runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            presenter.begin(runtime.view)
            runCurrent()
            presenter.edit(HandlingFields(observation = "Latest observation during native read"))
            gate.complete(Unit)
            runCurrent()
            presenter.confirm(true)
            presenter.park()
            assertTrue(presenter.state.value.needsReview)
            assertFalse(requireNotNull(presenter.state.value.draft).confirmed)
            runtime.review = runtime.review.copy(view = runtime.view.copy(stockRevision = 8UL))
            presenter.resume()
            runCurrent()
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(7UL))
            assertTrue(runtime.departureWrites.isEmpty())
            presenter.acceptReview()
            val draft = requireNotNull(presenter.state.value.draft)
            assertEquals("Latest observation during native read", draft.fields.observation)
            assertEquals(runtime.request, draft.request)
            assertEquals(runtime.view.record, draft.paper)
            assertEquals(8UL, draft.context?.view?.stockRevision)
            assertFalse(draft.confirmed)
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            assertEquals(
                8UL,
                runtime.departureWrites
                    .single()
                    .second.expectedStockRevision,
            )
            presenter.close()
        }

    @Test
    fun unknownOutcomeAndAbsentOriginalFreezeOneIntentUntilMatchingOriginalIsRecovered() =
        runTest {
            val runtime = DepartureFake()
            var allocated = 0
            val presenter =
                DeparturePresenter(
                    runtime,
                    this,
                    OperationIdSource {
                        allocated++
                        StockFixtures.operation
                    },
                )
            presenter.attach("hub")
            presenter.begin(runtime.view)
            runCurrent()
            presenter.edit(HandlingFields(observation = "Observed full departure"))
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            val frozen = assertIs<DepartureSubmission.Uncertain>(presenter.state.value.submission).attempt
            presenter.discard()
            presenter.edit(HandlingFields(observation = "Do not overwrite"))
            presenter.reviewRejected()
            presenter.submit(SnapshotRevision(9UL))
            presenter.checkOriginal()
            runCurrent()
            assertEquals(frozen, assertIs<DepartureSubmission.Uncertain>(presenter.state.value.submission).attempt)
            assertEquals(1, allocated)
            assertEquals(1, runtime.departureWrites.size)
            runtime.departureOriginal = runtime.result.copy(operationId = OperationId.from("01900000-0000-7000-8000-000000000099"))
            presenter.checkOriginal()
            runCurrent()
            assertIs<DepartureSubmission.Uncertain>(presenter.state.value.submission)
            runtime.departureOriginal = runtime.result
            presenter.checkOriginal()
            runCurrent()
            assertIs<DepartureSubmission.Uncertain>(presenter.state.value.submission)
            val matching = runtime.result.copy(observation = frozen.input.observation)
            runtime.departureOriginal = matching
            presenter.checkOriginal()
            runCurrent()
            assertEquals(matching, assertIs<DepartureSubmission.Recorded>(presenter.state.value.submission).receipt)
            assertEquals(runtime.view.record.id, presenter.finishRecorded())
            assertNull(presenter.state.value.draft)
            assertEquals(1, allocated)
            presenter.close()
        }

    @Test
    fun nativeBlockedReplacementCannotRetargetTheIssuedVersionOrEnableConfirmation() =
        runTest {
            val runtime = DepartureFake()
            val presenter = DeparturePresenter(runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            presenter.begin(runtime.view)
            runCurrent()
            presenter.edit(HandlingFields(observation = "Original observed facts"))
            runtime.review =
                runtime.review.copy(
                    ready = false,
                    block = conflict(),
                    view = runtime.view.copy(record = runtime.view.record.copy(revision = 3UL)),
                )
            presenter.reviewContext()
            runCurrent()
            presenter.acceptReview()
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(7UL))
            assertTrue(runtime.departureWrites.isEmpty())
            assertEquals(
                runtime.request,
                presenter.state.value.draft
                    ?.request,
            )
            assertEquals(
                2UL,
                presenter.state.value.draft
                    ?.paper
                    ?.revision,
            )
            assertFalse(requireNotNull(presenter.state.value.draft).confirmed)
            presenter.close()
        }

    @Test
    fun knownRejectionRequiresExplicitReviewAndLateNativeReadCannotCrossHubAttachment() =
        runTest {
            val runtime = DepartureFake()
            runtime.failure = ApplicationFailure(conflict())
            val presenter = DeparturePresenter(runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("old")
            presenter.begin(runtime.view)
            runCurrent()
            presenter.edit(HandlingFields(observation = "Retained facts after rejection"))
            presenter.confirm(true)
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            assertIs<DepartureSubmission.Rejected>(presenter.state.value.submission)
            presenter.reviewRejected()
            runCurrent()
            assertTrue(presenter.state.value.needsReview)
            assertEquals(
                "Retained facts after rejection",
                presenter.state.value.draft
                    ?.fields
                    ?.observation,
            )
            presenter.acceptReview()
            assertFalse(presenter.state.value.needsReview)
            assertFalse(requireNotNull(presenter.state.value.draft).confirmed)
            presenter.close()

            val gate = CompletableDeferred<Unit>()
            val delayed =
                object : DepartureFake() {
                    override suspend fun departureReview(input: HubDepartureReviewInput): HubDepartureReview =
                        withContext(NonCancellable) {
                            gate.await()
                            super.departureReview(input)
                        }
                }
            val next = DeparturePresenter(delayed, this, OperationIdSource { StockFixtures.operation })
            next.attach("old")
            next.begin(delayed.view)
            runCurrent()
            next.attach("new")
            gate.complete(Unit)
            runCurrent()
            assertNull(next.state.value.draft)
            assertNull(next.state.value.submission)
            assertFalse(next.state.value.loading)
            next.close()
        }
}

private fun conflict() =
    ApplicationProblem(
        ApplicationErrorCode.HubConflict,
        ApplicationErrorCategory.Conflict,
        false,
        RecoveryAction.None,
        null,
        "Current stock changed. Review the same identification.",
    )

internal open class DepartureFake :
    DispatchFake(),
    HarvestCircleDepartureRuntime {
    private val version =
        HubIdentificationVersion(
            "10".repeat(16),
            1U,
            null,
            requireNotNull(receipt.record.draft),
            receipt.record.updated,
            HubIdentificationStatus.Issued,
            null,
        )
    val view =
        HubDispatchView(
            4UL,
            5UL,
            receipt.record.copy(revision = 2UL, state = HubDispatchState.Issued, draft = null, versions = listOf(version)),
            HubDispatchState.Issued,
            null,
        )
    val request = HubDepartureReviewInput(view.record.id, view.record.revision, version.id)
    var review = HubDepartureReview(request, view, true, null, emptyList())
    val result =
        HubDepartureReceipt(
            "11".repeat(16),
            StockFixtures.operation,
            5UL,
            5UL,
            StockFixtures.actor,
            UnixSeconds(200),
            HubOccurrence(HubOccurrenceKind.Unknown),
            "Actual departure",
            view.record,
            emptyList(),
            emptyList(),
        )
    val departureWrites = mutableListOf<Pair<RequestContext, HubDepartureInput>>()
    var departureOriginal: HubDepartureReceipt? = null

    override suspend fun departureReview(input: HubDepartureReviewInput) = review.copy(request = input)

    override suspend fun applyDeparture(
        context: RequestContext,
        input: HubDepartureInput,
    ): HubDepartureReceipt {
        departureWrites += context to input
        failure?.let { throw it }
        error("Lost native departure reply")
    }

    override suspend fun departureOperation(operation: OperationId) = departureOriginal
}
