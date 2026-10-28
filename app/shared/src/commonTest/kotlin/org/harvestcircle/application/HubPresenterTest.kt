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
import kotlin.test.assertNull

@OptIn(ExperimentalCoroutinesApi::class)
class HubPresenterTest {
    @Test
    fun setupUsesOriginalIntentAndReadsDurableOutcomeAfterLostReply() =
        runTest {
            val runtime = FakeHubRuntime()
            val presenter = HubPresenter(runtime, this, OperationIdSource { setupId })
            presenter.bindIdentity(owner)
            runCurrent()
            assertEquals(HubViewState.NeedsSetup, presenter.state.value)
            presenter.initialize(SnapshotRevision(7UL))
            presenter.initialize(SnapshotRevision(7UL))
            runCurrent()
            assertEquals(setupId, assertIs<HubViewState.Failed>(presenter.state.value).submittedOperation)
            assertEquals(listOf(RequestContext(setupId, SnapshotRevision(7UL), 10_000UL)), runtime.submitted)
            presenter.initialize(SnapshotRevision(8UL))
            presenter.refresh()
            runCurrent()
            assertEquals(HubViewState.Ready(hub), presenter.state.value)
            assertEquals(1, runtime.submitted.size)
            presenter.close()
        }

    @Test
    fun lateResultCannotCrossIdentityOrSignOut() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : HarvestCircleHubRuntime {
                    override val demoProfile = true

                    override suspend fun hubContext(): HubContext? =
                        withContext(NonCancellable) {
                            gate.await()
                            hub
                        }

                    override suspend fun initializeHub(context: RequestContext): HubContext = error("Unexpected setup")
                }
            val presenter = HubPresenter(runtime, this, OperationIdSource { setupId })
            presenter.bindIdentity(owner)
            runCurrent()
            presenter.bindIdentity(null)
            gate.complete(Unit)
            runCurrent()
            assertEquals(HubViewState.SignedOut, presenter.state.value)
            presenter.close()
        }

    @Test
    fun ordinaryProfilesAndReadOnlyDoNotRequestHubReads() =
        runTest {
            val runtime = FakeHubRuntime()
            val presenter = HubPresenter(runtime, this, OperationIdSource { setupId })
            presenter.bindIdentity(null)
            presenter.refresh()
            runCurrent()
            assertEquals(0, runtime.reads)
            assertEquals(HubViewState.SignedOut, presenter.state.value)
            val ordinary = HubPresenter(null, this, OperationIdSource { setupId })
            ordinary.bindIdentity(owner)
            assertEquals(HubViewState.Unavailable, ordinary.state.value)
            assertNull(runtime.context)
            presenter.close()
            ordinary.close()
        }
}

private class FakeHubRuntime : HarvestCircleHubRuntime {
    override val demoProfile = true
    val submitted = mutableListOf<RequestContext>()
    var context: HubContext? = null
    var reads = 0

    override suspend fun hubContext(): HubContext? {
        reads += 1
        return context
    }

    override suspend fun initializeHub(context: RequestContext): HubContext {
        submitted += context
        this.context = hub
        error("Reply lost after commit")
    }
}

private val owner = IdentityId.fromPublicKeyHex("01".repeat(32))
private val hub = HubContext("00112233445566778899aabbccddeeff", owner, 1UL, true)
private val setupId = OperationId.from("01900000-0000-7000-8000-000000000001")
