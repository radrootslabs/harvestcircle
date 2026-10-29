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
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class LocalOutputPresenterTest {
    @Test
    fun cancellationFailureAndPrintRetryReuseExactlyOneNativeRendering() =
        runTest {
            val runtime = OutputRuntime()
            val port = OutputPort()
            val presenter = LocalOutputPresenter(runtime, port, this)
            presenter.attach("hub")
            presenter.prepare(outputRequest)
            runCurrent()
            val frozen = presenter.state.value.document
            port.result = LocalOutputResult.Cancelled
            presenter.perform(LocalOutputAction.SaveHtml)
            runCurrent()
            assertEquals(frozen, presenter.state.value.document)
            port.result = LocalOutputResult.Failed
            presenter.perform(LocalOutputAction.SaveHtml)
            runCurrent()
            port.result = LocalOutputResult.PrintSubmitted
            presenter.perform(LocalOutputAction.Print)
            runCurrent()
            assertEquals(listOf(outputRequest), runtime.requests)
            assertEquals(listOf(frozen, frozen, frozen), port.documents)
            assertTrue(
                presenter.state.value.message
                    .orEmpty()
                    .contains("Physical printing"),
            )
            presenter.close()
            assertEquals(1, port.closes)
        }

    @Test
    fun uncertainPrintRequiresQueueReviewAndSavingCannotClearThatRequirement() =
        runTest {
            val runtime = OutputRuntime()
            val port = OutputPort()
            val presenter = LocalOutputPresenter(runtime, port, this)
            presenter.attach("hub")
            presenter.prepare(outputRequest)
            runCurrent()
            port.result = LocalOutputResult.Uncertain
            presenter.perform(LocalOutputAction.Print)
            runCurrent()
            assertTrue(presenter.state.value.printNeedsReview)
            presenter.perform(LocalOutputAction.Print)
            runCurrent()
            assertEquals(1, port.documents.size)
            port.result = LocalOutputResult.Saved("chosen.html")
            presenter.perform(LocalOutputAction.SaveHtml)
            runCurrent()
            assertTrue(presenter.state.value.printNeedsReview)
            presenter.acknowledgePrintReview()
            presenter.perform(LocalOutputAction.Print)
            runCurrent()
            assertEquals(3, port.documents.size)
            assertEquals(1, runtime.requests.size)
            presenter.close()
        }

    @Test
    fun pendingOutputCannotRetargetAndDismissalRequestsOwnedCancellationUntilItSettles() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val port =
                object : OutputPort() {
                    override suspend fun perform(
                        action: LocalOutputAction,
                        document: HubLocalOutput,
                    ): LocalOutputResult {
                        gate.await()
                        return super.perform(action, document)
                    }
                }
            val runtime = OutputRuntime()
            val presenter = LocalOutputPresenter(runtime, port, this)
            presenter.attach("hub")
            presenter.prepare(outputRequest)
            runCurrent()
            presenter.perform(LocalOutputAction.SaveHtml)
            runCurrent()
            presenter.prepare(HubLocalOutputRequest.Identification("other", "other"))
            val cancellations = port.cancellations
            assertTrue(presenter.dismiss())
            assertTrue(presenter.state.value.open)
            assertTrue(presenter.state.value.busy)
            assertEquals(cancellations + 1, port.cancellations)
            gate.complete(Unit)
            runCurrent()
            assertFalse(presenter.state.value.busy)
            assertTrue(presenter.dismiss())
            assertFalse(presenter.state.value.open)
            assertEquals(listOf(outputRequest), runtime.requests)
            presenter.close()
        }

    @Test
    fun lateNativeRenderingCannotCrossHubOrClosedOwner() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            val runtime =
                object : OutputRuntime() {
                    override suspend fun localOutput(request: HubLocalOutputRequest): HubLocalOutput =
                        withContext(NonCancellable) {
                            gate.await()
                            super.localOutput(request)
                        }
                }
            val port = OutputPort()
            val presenter = LocalOutputPresenter(runtime, port, this)
            presenter.attach("first")
            presenter.prepare(outputRequest)
            runCurrent()
            presenter.attach("second")
            gate.complete(Unit)
            runCurrent()
            assertNull(presenter.state.value.document)
            assertFalse(presenter.state.value.open)
            presenter.close()
            presenter.prepare(outputRequest)
            runCurrent()
            assertFalse(presenter.state.value.available)
        }
}

private val outputRequest: HubLocalOutputRequest = HubLocalOutputRequest.Identification("preparation", "issued-version")

private open class OutputRuntime : HarvestCircleTraceRuntime {
    val requests = mutableListOf<HubLocalOutputRequest>()

    override suspend fun localOutput(request: HubLocalOutputRequest): HubLocalOutput {
        requests += request
        return HubLocalOutput("captured-content-identity", "captured.html", "<html><body>Captured Ω</body></html>")
    }

    override suspend fun tracePage(query: HubTraceQuery): HubTracePage = error("Not an output operation")

    override suspend fun traceSnapshot(query: HubTraceSnapshotQuery): HubTraceSnapshot = error("Not an output operation")

    override suspend fun traceHistory(query: HubTraceHistoryQuery): HubTraceHistoryPage = error("Not an output operation")
}

private open class OutputPort : LocalOutputPort {
    val documents = mutableListOf<HubLocalOutput>()
    var result: LocalOutputResult = LocalOutputResult.Cancelled
    var cancellations = 0
    var closes = 0

    override suspend fun perform(
        action: LocalOutputAction,
        document: HubLocalOutput,
    ): LocalOutputResult {
        documents += document
        return result
    }

    override fun cancel() {
        cancellations += 1
    }

    override fun close() {
        closes += 1
    }
}
