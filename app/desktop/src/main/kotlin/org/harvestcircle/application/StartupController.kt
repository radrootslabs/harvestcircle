package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import org.harvestcircle.application.generated.DesktopBuildMetadata
import java.security.SecureRandom
import java.util.HexFormat

internal sealed interface StartupState<out T> {
    data object Opening : StartupState<Nothing>

    data class Ready<T>(
        val value: T,
    ) : StartupState<T>

    data class Failed(
        val problem: ApplicationProblem,
        val diagnostics: String,
        val canRetry: Boolean,
    ) : StartupState<Nothing>

    data object Closing : StartupState<Nothing>

    data object CloseFailed : StartupState<Nothing>

    data object Closed : StartupState<Nothing>
}

/** Owns opening even when a synchronous native call outlives its UI consumer. */
internal class StartupController<T : Any>(
    private val applicationScope: CoroutineScope,
    private val open: (CoroutineScope) -> T,
    private val close: suspend (T) -> Boolean,
    private val timeoutMillis: Long,
    dispatcher: CoroutineDispatcher = Dispatchers.IO,
) {
    private val owner = CoroutineScope(SupervisorJob() + dispatcher)
    private val lock = Any()
    private val mutableState = MutableStateFlow<StartupState<T>>(StartupState.Opening)
    val state = mutableState.asStateFlow()
    private var running = false
    private var closing = false
    private var value: T? = null
    private var attemptId = "unstarted"
    private var retainedCleanup: (suspend () -> Boolean)? = null

    fun start() =
        synchronized(lock) {
            if (running || closing || value != null || (mutableState.value as? StartupState.Failed)?.canRetry == false) return@synchronized
            running = true
            mutableState.value = StartupState.Opening
            val attempt = HexFormat.of().formatHex(ByteArray(16).also(startupRandom::nextBytes))
            attemptId = attempt
            owner.launch {
                var failure: ApplicationProblem? = null
                try {
                    val opened = open(applicationScope)
                    synchronized(lock) {
                        value = opened
                        if (!closing) mutableState.value = StartupState.Ready(opened)
                    }
                } catch (error: CancellationException) {
                    requestClose()
                    throw error
                } catch (error: StartupConstructionFailure) {
                    retainedCleanup = error.cleanup
                    val cleaned =
                        try {
                            withTimeoutOrNull(timeoutMillis) { error.cleanup() } == true
                        } catch (cancelled: CancellationException) {
                            synchronized(lock) {
                                closing = true
                                reportCloseFailure()
                            }
                            throw cancelled
                        } catch (_: Exception) {
                            false
                        }
                    if (cleaned) {
                        retainedCleanup = null
                        if (error.original is CancellationException) {
                            requestClose()
                            throw error.original
                        }
                        failure = error.original.toStartupProblem()
                    } else {
                        synchronized(lock) {
                            closing = true
                            reportCloseFailure()
                        }
                    }
                } catch (error: Exception) {
                    failure = error.toStartupProblem()
                } catch (error: LinkageError) {
                    failure = error.toStartupProblem()
                } finally {
                    synchronized(lock) {
                        running = false
                        failure?.let { problem ->
                            val diagnostics = startupDiagnostics(attempt, problem)
                            System.err.println(diagnostics)
                            mutableState.value = StartupState.Failed(problem, diagnostics, problem.permitsStartupRetry())
                        }
                        if (closing && retainedCleanup == null) beginClose()
                    }
                }
            }
        }

    fun requestClose() =
        synchronized(lock) {
            if (closing) return@synchronized
            closing = true
            mutableState.value = StartupState.Closing
            if (!running) {
                beginClose()
            } else {
                owner.launch {
                    delay(timeoutMillis)
                    synchronized(lock) {
                        if (running && mutableState.value == StartupState.Closing) {
                            reportCloseFailure()
                        }
                    }
                }
            }
        }

    init {
        require(timeoutMillis > 0)
    }

    private fun reportCloseFailure() {
        System.err.println(
            "harvestcircle.startup stage=CLOSE code=Internal attempt=$attemptId build=${DesktopBuildMetadata.productVersion}",
        )
        mutableState.value = StartupState.CloseFailed
    }

    private fun beginClose() {
        if (running) return
        running = true
        val opened = value
        owner.launch {
            val closed =
                try {
                    opened == null || withTimeoutOrNull(timeoutMillis) { close(opened) } == true
                } catch (error: CancellationException) {
                    synchronized(lock) { reportCloseFailure() }
                    throw error
                } catch (_: Exception) {
                    false
                }
            synchronized(lock) {
                running = false
                if (closed) {
                    value = null
                    applicationScope.cancel()
                    mutableState.value = StartupState.Closed
                    owner.cancel()
                } else {
                    // Retain ownership; no retry may open another native runtime.
                    reportCloseFailure()
                }
            }
        }
    }
}

internal fun Throwable.toStartupProblem(): ApplicationProblem =
    when (this) {
        is ApplicationFailure -> problem
        is NativeCompatibilityException ->
            startupProblem(
                ApplicationErrorCode.CompatibilityMismatch,
                RecoveryAction.UpdateApplication,
                "The application and native runtime are incompatible. Rebuild or reinstall HarvestCircle.",
            )
        is LinkageError ->
            startupProblem(
                ApplicationErrorCode.NativeLibraryLoadFailed,
                RecoveryAction.UpdateApplication,
                "The native runtime could not load. Rebuild or reinstall HarvestCircle.",
            )
        is org.harvestcircle.ffi.HarvestCircleException.Failure ->
            toApplicationProblem().let { native ->
                if (native.code == ApplicationErrorCode.StorageUnavailable) {
                    native.copy(
                        retryable = false,
                        recoveryAction = RecoveryAction.CheckConfiguration,
                        safeMessage =
                            "HarvestCircle cannot open its local data. Check its availability and permissions, " +
                                "and close other HarvestCircle instances before trying again.",
                    )
                } else {
                    native
                }
            }
        else ->
            startupProblem(
                ApplicationErrorCode.Internal,
                RecoveryAction.None,
                "HarvestCircle could not start. Copy the diagnostics when reporting this problem.",
            )
    }

private fun startupProblem(
    code: ApplicationErrorCode,
    recovery: RecoveryAction,
    message: String,
) = ApplicationProblem(code, ApplicationErrorCategory.Lifecycle, false, recovery, null, message)

internal fun ApplicationProblem.permitsStartupRetry(): Boolean = code == ApplicationErrorCode.StorageUnavailable

internal fun startupDiagnostics(
    attempt: String,
    problem: ApplicationProblem,
): String = "harvestcircle.startup stage=OPEN code=${problem.code.name} attempt=$attempt build=${DesktopBuildMetadata.productVersion}"

internal class StartupConstructionFailure(
    val original: Throwable,
    val cleanup: suspend () -> Boolean,
) : Exception("Startup construction failed")

private val startupRandom = SecureRandom()
