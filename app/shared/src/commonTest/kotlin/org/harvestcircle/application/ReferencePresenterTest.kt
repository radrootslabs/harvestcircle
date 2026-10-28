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
class ReferencePresenterTest {
    @Test
    fun lostReplyFreezesOriginalIntentAndOnlyReadsItsResult() =
        runTest {
            val runtime = ReferenceFake()
            var ids = 0
            val presenter =
                ReferencePresenter(
                    runtime,
                    this,
                    OperationIdSource {
                        ids += 1
                        referenceOperation
                    },
                )
            presenter.attach("hub")
            runCurrent()
            presenter.create()
            presenter.updateDraft { it.copy(label = "Carrots") }
            presenter.submit(SnapshotRevision(7UL))
            presenter.submit(SnapshotRevision(7UL))
            presenter.updateDraft { it.copy(label = "Changed") }
            presenter.inspect(referenceRecord)
            presenter.closeInspection()
            runCurrent()
            val unknown = assertIs<ReferenceSubmission.Uncertain>(presenter.state.value.submission)
            assertEquals("Carrots", unknown.attempt.edit.value.label)
            assertEquals(
                "Carrots",
                presenter.state.value.draft
                    ?.label,
            )
            assertEquals(1, ids)
            assertEquals(1, runtime.writes.size)
            presenter.checkOriginal()
            runCurrent()
            val recorded = assertIs<ReferenceSubmission.Recorded>(presenter.state.value.submission)
            assertEquals(referenceOperation, recorded.receipt.operationId)
            assertEquals(listOf(referenceOperation), runtime.lookups)
            assertEquals(1, runtime.writes.size)
            presenter.finishRecorded()
            runCurrent()
            assertNull(presenter.state.value.draft)
            presenter.close()
        }

    @Test
    fun absentReceiptAfterRestoreCannotAuthorizeNewSubmission() =
        runTest {
            val runtime = ReferenceFake().apply { stored = null }
            val presenter = ReferencePresenter(runtime, this, OperationIdSource { referenceOperation })
            presenter.attach("hub")
            runCurrent()
            presenter.create()
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            runtime.stored = null
            presenter.checkOriginal()
            runCurrent()
            assertIs<ReferenceSubmission.Uncertain>(presenter.state.value.submission)
            presenter.reviewRejected()
            presenter.closeDraft()
            presenter.create()
            presenter.submit(SnapshotRevision(8UL))
            runCurrent()
            assertEquals(1, runtime.writes.size)
            assertEquals(
                referenceOperation,
                presenter.state.value.submission
                    ?.attempt
                    ?.operation,
            )
            presenter.close()
        }

    @Test
    fun conflictRetainsInputAndRequiresExplicitReviewOfCurrentVersions() =
        runTest {
            val runtime = ReferenceFake().apply { failure = referenceConflict() }
            val presenter = ReferencePresenter(runtime, this, OperationIdSource { referenceOperation })
            presenter.attach("hub")
            runCurrent()
            presenter.edit(referenceRecord)
            presenter.updateDraft { it.copy(label = "My retained name") }
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            assertIs<ReferenceSubmission.Rejected>(presenter.state.value.submission)
            presenter.reviewRejected()
            presenter.submit(SnapshotRevision(8UL))
            runCurrent()
            assertEquals(1, runtime.writes.size)
            assertTrue(presenter.state.value.draftNeedsReview)
            runtime.page =
                referencePage.copy(
                    catalogRevision = 9UL,
                    records = listOf(referenceRecord.copy(revision = 2UL, value = referenceRecord.value.copy(label = "Other name"))),
                )
            presenter.reviewDraftContext()
            runCurrent()
            assertEquals(
                "Other name",
                presenter.state.value.contextReview
                    ?.current
                    ?.value
                    ?.label,
            )
            assertEquals(
                1UL,
                presenter.state.value.draft
                    ?.recordRevision,
            )
            presenter.acceptContextReview()
            assertEquals(
                "My retained name",
                presenter.state.value.draft
                    ?.label,
            )
            assertEquals(
                9UL,
                presenter.state.value.draft
                    ?.catalogRevision,
            )
            assertEquals(
                2UL,
                presenter.state.value.draft
                    ?.recordRevision,
            )
            assertFalse(presenter.state.value.draftNeedsReview)
            presenter.close()
        }

    @Test
    fun lateUncancelableReadCannotRestoreClosedIdentityState() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : ReferenceFake() {
                    override suspend fun references(query: HubReferenceQuery): HubReferencePage =
                        withContext(NonCancellable) {
                            gate.await()
                            referencePage
                        }
                }
            val presenter = ReferencePresenter(runtime, this, OperationIdSource { referenceOperation })
            presenter.attach("hub")
            runCurrent()
            presenter.close()
            gate.complete(Unit)
            runCurrent()
            assertEquals(ReferenceViewState(), presenter.state.value)
        }

    @Test
    fun failedNewCategoryReadDoesNotRelabelOldResults() =
        runTest {
            val runtime =
                object : ReferenceFake() {
                    override suspend fun references(query: HubReferenceQuery): HubReferencePage {
                        if (query.kind == HubReferenceKind.Party) error("Unavailable")
                        return referencePage
                    }
                }
            val presenter = ReferencePresenter(runtime, this, OperationIdSource { referenceOperation })
            presenter.attach("hub")
            runCurrent()
            assertEquals(referencePage, presenter.state.value.page)
            presenter.selectKind(HubReferenceKind.Party)
            runCurrent()
            assertNull(presenter.state.value.page)
            presenter.create()
            assertNull(presenter.state.value.draft)
            presenter.close()
        }

    @Test
    fun nextPageRetainsLoadedSearchAndNativeRevisionDespiteUnsubmittedSearchInput() =
        runTest {
            val runtime = ReferenceFake().apply { page = referencePage.copy(more = true, total = 2U) }
            val presenter = ReferencePresenter(runtime, this, OperationIdSource { referenceOperation })
            presenter.attach("hub")
            runCurrent()
            presenter.search("carrot")
            presenter.reload()
            runCurrent()
            presenter.search("unsubmitted")
            presenter.nextPage()
            runCurrent()
            assertEquals(HubReferenceQuery(1UL, 1U, kind = HubReferenceKind.Product, text = "carrot"), runtime.queries.last())
            presenter.inspect(referenceRecord)
            runCurrent()
            assertEquals(Triple(referenceRecord.id, 0UL, 64U), runtime.histories.single())
            presenter.close()
        }
}

private open class ReferenceFake : HarvestCircleReferenceRuntime {
    var page = referencePage
    var stored: HubReferenceReceipt? = null
    var failure: Exception? = null
    val queries = mutableListOf<HubReferenceQuery>()
    val writes = mutableListOf<Pair<RequestContext, HubReferenceEdit>>()
    val lookups = mutableListOf<OperationId>()
    val histories = mutableListOf<Triple<String, ULong, UInt>>()

    override suspend fun references(query: HubReferenceQuery): HubReferencePage {
        queries += query
        return page
    }

    override suspend fun editReference(
        context: RequestContext,
        edit: HubReferenceEdit,
    ): HubReferenceReceipt {
        writes += context to edit
        failure?.let { throw it }
        stored =
            HubReferenceReceipt(
                context.operationId,
                referenceActor,
                UnixSeconds(100L),
                2UL,
                null,
                referenceRecord.copy(value = edit.value),
            )
        error("Reply lost after native commit")
    }

    override suspend fun referenceOperation(operation: OperationId): HubReferenceReceipt? {
        lookups += operation
        return stored
    }

    override suspend fun referenceHistory(
        id: String,
        afterRevision: ULong,
        limit: UInt,
    ): List<HubReferenceReceipt> {
        histories += Triple(id, afterRevision, limit)
        return listOfNotNull(stored)
    }
}

private fun referenceConflict() =
    ApplicationFailure(
        ApplicationProblem(
            ApplicationErrorCode.HubConflict,
            ApplicationErrorCategory.Conflict,
            false,
            RecoveryAction.None,
            referenceOperation,
            "Reference changed",
        ),
    )

private val referenceOperation = OperationId.from("01900000-0000-7000-8000-000000000021")
private val referenceActor = IdentityId.fromPublicKeyHex("01".repeat(32))
private val referenceRecord = HubReferenceRecord("01".repeat(16), 1UL, HubReferenceValue(HubReferenceKind.Product, "Carrots"))
private val referencePage =
    HubReferencePage(
        HubHandlingBasis("whole-pack-demo-v1", 1U, "Whole-pack demo", "UTC"),
        1UL,
        0U,
        1U,
        false,
        listOf(referenceRecord),
    )
