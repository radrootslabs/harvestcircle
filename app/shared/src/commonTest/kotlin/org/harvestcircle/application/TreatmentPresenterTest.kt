package org.harvestcircle.application

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull

@OptIn(ExperimentalCoroutinesApi::class)
class TreatmentPresenterTest {
    @Test
    fun repeatedPhysicalChoicesFailBeforeOpeningATreatmentDraft() =
        runTest {
            val runtime =
                object : HandlingFake() {
                    override suspend fun lineage(query: HubLineageQuery): HubLineagePage =
                        super.lineage(query).copy(total = 2U, offset = query.offset, more = query.offset == 0U)
                }
            val presenter = HandlingPresenter(runtime, runtime, this, OperationIdSource { StockFixtures.operation })
            presenter.attach("hub")
            runCurrent()
            presenter.inspectGroup(HandlingFixtures.group.group.id)
            runCurrent()
            presenter.begin(HandlingAction.SetAcceptance)
            assertNotNull(presenter.state.value.message)
            assertNull(presenter.state.value.group)
            assertNull(presenter.state.value.treatment)
            assertNull(presenter.state.value.draft)
            assertEquals(0, runtime.writes.size)
            presenter.close()
        }

    @Test
    fun acceptanceKeepsExplicitSelectionAndFreezesUncertainOriginalIntent() =
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
            presenter.begin(HandlingAction.SetAcceptance)
            presenter.edit(
                requireNotNull(
                    presenter.state.value.draft,
                ).fields.copy(acceptanceStatus = HubAcceptanceStatus.Accepted, observation = "Sample inspection explicitly accepted"),
            )
            presenter.parkDraft()
            presenter.chooseTab(HandlingTab.Discrepancies)
            runCurrent()
            presenter.begin(HandlingAction.PlaceHold)
            val draft = assertIs<HandlingDraft.Treatment>(presenter.state.value.draft)
            assertEquals(TreatmentEditorAction.Acceptance, draft.action)
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            val frozen = assertIs<HandlingSubmission.Uncertain>(presenter.state.value.submission)
            val input = assertIs<HubHandlingInput.SetAcceptance>(frozen.attempt.input)
            assertEquals(listOf(HandlingFixtures.group.group.id), input.groupIds)
            assertEquals(HubAcceptanceStatus.Accepted, input.status)
            assertEquals(HubOccurrence(HubOccurrenceKind.Unknown), input.occurrence)
            presenter.edit(draft.fields.copy(acceptanceStatus = HubAcceptanceStatus.Rejected))
            presenter.submit(SnapshotRevision(8UL))
            runCurrent()
            assertEquals(1, ids)
            assertEquals(1, runtime.writes.size)
            assertEquals(
                HubAcceptanceStatus.Accepted,
                presenter.state.value.draft
                    ?.fields
                    ?.acceptanceStatus,
            )
            presenter.close()
        }

    @Test
    fun lateTreatmentReadCannotRestoreAnotherHubPopulation() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : HandlingFake() {
                    override suspend fun treatment(scope: HubTreatmentScope): HubTreatmentSnapshot =
                        withContext(NonCancellable) {
                            gate.await()
                            super.treatment(scope)
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
            assertNull(presenter.state.value.treatment)
            assertNull(presenter.state.value.treatmentHistory)
            assertNull(presenter.state.value.group)
            presenter.close()
        }

    @Test
    fun releaseRequiresAnExplicitExistingHoldBeforeAllocatingOperation() =
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
            presenter.begin(HandlingAction.ReleaseHold)
            presenter.submit(SnapshotRevision(7UL))
            runCurrent()
            assertNotNull(presenter.state.value.message)
            assertNull(presenter.state.value.submission)
            assertEquals(0, ids)
            assertEquals(0, runtime.writes.size)
            presenter.close()
        }
}
