package org.harvestcircle.desktop

import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.application.HarvestCircleApplication
import org.harvestcircle.application.NativeHarvestCircleRuntime
import org.harvestcircle.application.verifyNativeCompatibility
import org.harvestcircle.ffi.HarvestCircleException
import org.harvestcircle.ffi.compatibilityDescriptor
import org.harvestcircle.identities.ui.StartupFailureScreen
import java.awt.Dimension
import java.awt.Taskbar
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import javax.imageio.ImageIO
import kotlin.system.exitProcess

private const val APPLICATION_NAME = "HarvestCircle"
internal const val INITIAL_WINDOW_WIDTH = 1280
internal const val INITIAL_WINDOW_HEIGHT = 800
internal const val MINIMUM_WINDOW_WIDTH = 1100
internal const val MINIMUM_WINDOW_HEIGHT = 720
internal const val HEALTH_CHECK_ARGUMENT = "--health-check"
internal const val HEALTH_READY_EVIDENCE = "HARVESTCIRCLE_HEALTH_READY"
internal const val HEALTH_CLOSED_EVIDENCE = "HARVESTCIRCLE_HEALTH_CLOSED"
private const val HEALTH_FAILURE_EVIDENCE = "HARVESTCIRCLE_HEALTH_FAILED"
private const val HEALTH_TIMEOUT_MILLIS = 90_000L

private val isMacOs: Boolean =
    System
        .getProperty("os.name", "")
        .startsWith("Mac", ignoreCase = true)

fun main(args: Array<String>) {
    if (isHealthCheck(args)) exitProcess(runHealthCheck())

    val nativeStartupProblem = if (isMacOs) configureMacOsApplication() else null

    application {
        var closeRequested by remember { mutableStateOf(false) }
        Window(
            onCloseRequest = {
                if (nativeStartupProblem == null) closeRequested = true else exitApplication()
            },
            title = APPLICATION_NAME,
            state =
                rememberWindowState(
                    width = INITIAL_WINDOW_WIDTH.dp,
                    height = INITIAL_WINDOW_HEIGHT.dp,
                ),
        ) {
            DisposableEffect(window) {
                window.minimumSize = Dimension(MINIMUM_WINDOW_WIDTH, MINIMUM_WINDOW_HEIGHT)

                if (isMacOs) {
                    val rootPane = window.rootPane
                    rootPane.putClientProperty("apple.awt.fullWindowContent", true)
                    rootPane.putClientProperty("apple.awt.transparentTitleBar", true)
                    rootPane.putClientProperty("apple.awt.windowTitleVisible", false)
                }

                onDispose { }
            }

            if (nativeStartupProblem == null) {
                HarvestCircleApplication(
                    closeRequested = closeRequested,
                    onExitApproved = ::exitApplication,
                )
            } else {
                StartupFailureScreen(nativeStartupProblem)
            }
        }
    }
}

internal fun isHealthCheck(args: Array<String>): Boolean = args.size == 1 && args.single() == HEALTH_CHECK_ARGUMENT

private fun runHealthCheck(): Int {
    val executor = Executors.newSingleThreadExecutor()
    return try {
        executor
            .submit<Int> { runBlocking { executeHealthCheck() } }
            .get(HEALTH_TIMEOUT_MILLIS, TimeUnit.MILLISECONDS)
    } catch (_: TimeoutException) {
        System.err.println("$HEALTH_FAILURE_EVIDENCE:TIMEOUT")
        1
    } finally {
        executor.shutdownNow()
    }
}

private suspend fun executeHealthCheck(): Int {
    var runtime: NativeHarvestCircleRuntime? = null
    var closed = false
    var stage = "OPEN"
    try {
        withTimeout(HEALTH_TIMEOUT_MILLIS) {
            stage = "COMPATIBILITY"
            verifyNativeCompatibility(compatibilityDescriptor())
            stage = "OPEN"
            runtime = NativeHarvestCircleRuntime.open(developmentMode = true)
            stage = "BOOTSTRAP"
            val snapshot = requireNotNull(runtime).bootstrap()
            stage = "READY"
            require(snapshot.lifecycle in setOf(ApplicationLifecycle.Ready, ApplicationLifecycle.Degraded))
            println(HEALTH_READY_EVIDENCE)
            stage = "SHUTDOWN"
            val receipt = requireNotNull(runtime).shutdown()
            require(receipt.closed)
            closed = true
            println(HEALTH_CLOSED_EVIDENCE)
        }
        return 0
    } catch (error: HarvestCircleException.Failure) {
        System.err.println("$HEALTH_FAILURE_EVIDENCE:$stage:${error.code}:${error.safeMessage}")
        return 1
    } catch (error: Exception) {
        System.err.println("$HEALTH_FAILURE_EVIDENCE:$stage:${error.javaClass.simpleName}")
        return 1
    } finally {
        if (!closed) {
            runCatching {
                withTimeout(HEALTH_TIMEOUT_MILLIS) { runtime?.shutdown() }
            }
        }
    }
}

private fun configureMacOsApplication(): String? {
    System.setProperty("apple.awt.application.name", APPLICATION_NAME)
    System.setProperty("apple.awt.application.appearance", "system")

    if (!Taskbar.isTaskbarSupported()) return null

    val taskbar = Taskbar.getTaskbar()
    if (!taskbar.isSupported(Taskbar.Feature.ICON_IMAGE)) return null

    val icon =
        loadRuntimeIcon {
            Thread.currentThread().contextClassLoader.getResourceAsStream("icons/harvestcircle.png")
        } ?: return "The application icon resource is unavailable."
    return runCatching { taskbar.iconImage = icon }
        .fold(
            onSuccess = { null },
            onFailure = { "The application icon could not be configured." },
        )
}

internal fun loadRuntimeIcon(openResource: () -> java.io.InputStream?): java.awt.Image? =
    runCatching { openResource()?.use(ImageIO::read) }.getOrNull()
