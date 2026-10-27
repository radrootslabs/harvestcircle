package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withTimeout
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

class StartupControllerTest {
    @Test
    fun closeDuringBlockingOpenClosesTheLateResultExactlyOnce() =
        runBlocking {
            val entered = CountDownLatch(1)
            val release = CountDownLatch(1)
            val scope = CoroutineScope(SupervisorJob())
            var closes = 0
            val controller =
                StartupController(scope, {
                    entered.countDown()
                    check(release.await(5, TimeUnit.SECONDS))
                    "native handle"
                }, {
                    closes += 1
                    true
                }, 20)
            try {
                controller.start()
                assertTrue(entered.await(5, TimeUnit.SECONDS))
                controller.requestClose()
                controller.requestClose()
                controller.start()
                assertEquals(StartupState.Closing, controller.state.value)
                withTimeout(5_000) { controller.state.first { it == StartupState.CloseFailed } }
                release.countDown()
                withTimeout(5_000) { controller.state.first { it == StartupState.Closed } }
                assertEquals(1, closes)
            } finally {
                release.countDown()
                controller.requestClose()
            }
        }

    @Test
    fun retryIsDeliberateAndOnlyAvailableAfterThePreviousAttemptFinished() =
        runTest {
            val dispatcher = StandardTestDispatcher(testScheduler)
            val scope = CoroutineScope(SupervisorJob() + dispatcher)
            var attempts = 0
            val controller =
                StartupController(scope, {
                    attempts += 1
                    if (attempts == 1) throw ApplicationFailure(storageProblem())
                    "ready"
                }, { true }, 5_000, dispatcher)
            controller.start()
            controller.start()
            testScheduler.runCurrent()
            val failure = assertIs<StartupState.Failed>(controller.state.value)
            assertTrue(failure.canRetry)
            assertEquals(1, attempts)
            controller.start()
            testScheduler.runCurrent()
            assertIs<StartupState.Ready<String>>(controller.state.value)
            assertEquals(2, attempts)
            controller.requestClose()
            testScheduler.advanceUntilIdle()
            assertEquals(StartupState.Closed, controller.state.value)
        }

    @Test
    fun constructionFailureWaitsForConfirmedCleanupBeforeOfferingRetry() =
        runTest {
            val dispatcher = StandardTestDispatcher(testScheduler)
            val scope = CoroutineScope(SupervisorJob() + dispatcher)
            val cleanup = CompletableDeferred<Boolean>()
            var attempts = 0
            val controller =
                StartupController(scope, {
                    attempts += 1
                    if (attempts == 1) throw StartupConstructionFailure(ApplicationFailure(storageProblem())) { cleanup.await() }
                    "ready"
                }, { true }, 5_000, dispatcher)
            controller.start()
            testScheduler.runCurrent()
            assertEquals(StartupState.Opening, controller.state.value)
            controller.start()
            assertEquals(1, attempts)
            cleanup.complete(true)
            testScheduler.runCurrent()
            assertTrue(assertIs<StartupState.Failed>(controller.state.value).canRetry)
            controller.start()
            testScheduler.runCurrent()
            assertIs<StartupState.Ready<String>>(controller.state.value)
            controller.requestClose()
            testScheduler.advanceUntilIdle()
        }

    @Test
    fun failedConstructionCleanupBlocksEveryFurtherOpen() =
        runTest {
            val dispatcher = StandardTestDispatcher(testScheduler)
            val scope = CoroutineScope(SupervisorJob() + dispatcher)
            var attempts = 0
            var cleanups = 0
            val controller =
                StartupController<String>(scope, {
                    attempts += 1
                    throw StartupConstructionFailure(IllegalStateException("private")) {
                        cleanups += 1
                        false
                    }
                }, { true }, 5_000, dispatcher)
            controller.start()
            testScheduler.advanceUntilIdle()
            assertEquals(StartupState.CloseFailed, controller.state.value)
            controller.start()
            controller.requestClose()
            testScheduler.advanceUntilIdle()
            assertEquals(1, attempts)
            assertEquals(1, cleanups)
            scope.cancel()
        }

    @Test
    fun cancelledOpeningDoesNotBecomeAnInternalFailure() =
        runTest {
            val dispatcher = StandardTestDispatcher(testScheduler)
            val scope = CoroutineScope(SupervisorJob() + dispatcher)
            val controller = StartupController<String>(scope, { throw CancellationException() }, { true }, 5_000, dispatcher)
            controller.start()
            testScheduler.advanceUntilIdle()
            assertEquals(StartupState.Closed, controller.state.value)
        }

    @Test
    fun cancelledShutdownRetainsOwnershipAndAllowsExplicitForceExit() =
        runTest {
            val dispatcher = StandardTestDispatcher(testScheduler)
            val scope = CoroutineScope(SupervisorJob() + dispatcher)
            val controller = StartupController(scope, { "handle" }, { throw CancellationException() }, 5_000, dispatcher)
            controller.start()
            testScheduler.runCurrent()
            controller.requestClose()
            testScheduler.advanceUntilIdle()
            assertEquals(StartupState.CloseFailed, controller.state.value)
            controller.start()
            assertEquals(StartupState.CloseFailed, controller.state.value)
            scope.cancel()
        }

    @Test
    fun errorsAndDiagnosticsExcludeRawMessagesAndNativeOperationIds() {
        val secret = "/private/nsec1-sensitive"
        val problem = IllegalStateException(secret).toStartupProblem()
        assertEquals(ApplicationErrorCode.Internal, problem.code)
        assertFalse(problem.safeMessage.contains(secret))
        assertFalse(startupDiagnostics("host-attempt", problem).contains(secret))
        assertEquals(ApplicationErrorCode.NativeLibraryLoadFailed, UnsatisfiedLinkError(secret).toStartupProblem().code)
        assertEquals(ApplicationErrorCode.CompatibilityMismatch, NativeCompatibilityException().toStartupProblem().code)
        assertFalse(problem.permitsStartupRetry())
    }
}

private fun storageProblem() =
    ApplicationProblem(
        ApplicationErrorCode.StorageUnavailable,
        ApplicationErrorCategory.Storage,
        false,
        RecoveryAction.CheckConfiguration,
        null,
        "Check local data permissions before trying again.",
    )
