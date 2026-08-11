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
import org.harvestcircle.application.desktopRuntimeOpenConfiguration
import org.harvestcircle.application.verifyNativeCompatibility
import org.harvestcircle.ffi.HarvestCircleException
import org.harvestcircle.ffi.compatibilityDescriptor
import org.harvestcircle.identities.ui.StartupFailureScreen
import java.awt.Dimension
import java.awt.Taskbar
import java.nio.file.FileVisitResult
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.Path
import java.nio.file.SimpleFileVisitor
import java.nio.file.StandardOpenOption
import java.nio.file.attribute.BasicFileAttributes
import java.security.SecureRandom
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
internal const val HEALTH_DATA_ENVIRONMENT = "HARVESTCIRCLE_DEVELOPMENT_DATA_DIR"
private const val HEALTH_FAILURE_EVIDENCE = "HARVESTCIRCLE_HEALTH_FAILED"
private const val HEALTH_TIMEOUT_MILLIS = 90_000L
private const val HEALTH_OWNER_MARKER = ".harvestcircle-health-owner-v1"
private const val HEALTH_OWNER_TOKEN_BYTES = 16
private val healthOwnershipRandom = SecureRandom()

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
        val healthCheck =
            executor.submit<Int> {
                runBlocking {
                    withTimeout(HEALTH_TIMEOUT_MILLIS) {
                        executeHealthCheck(System.getenv(HEALTH_DATA_ENVIRONMENT))
                    }
                }
            }
        healthCheck.get(HEALTH_TIMEOUT_MILLIS, TimeUnit.MILLISECONDS)
    } catch (_: TimeoutException) {
        System.err.println("$HEALTH_FAILURE_EVIDENCE:TIMEOUT")
        1
    } finally {
        executor.shutdownNow()
    }
}

internal interface PackagedHealthRuntime {
    suspend fun bootstrapLifecycle(): ApplicationLifecycle

    suspend fun shutdownClosed(): Boolean
}

internal fun interface PackagedHealthRuntimeOpener {
    fun open(dataRoot: Path): PackagedHealthRuntime
}

private class NativePackagedHealthRuntime(
    private val runtime: NativeHarvestCircleRuntime,
) : PackagedHealthRuntime {
    override suspend fun bootstrapLifecycle(): ApplicationLifecycle = runtime.bootstrap().lifecycle

    override suspend fun shutdownClosed(): Boolean = runtime.shutdown().closed
}

internal suspend fun executeHealthCheck(
    developmentDataDirectory: String?,
    timeoutMillis: Long = HEALTH_TIMEOUT_MILLIS,
    runtimeOpener: PackagedHealthRuntimeOpener =
        PackagedHealthRuntimeOpener {
            NativePackagedHealthRuntime(
                NativeHarvestCircleRuntime.open(
                    desktopRuntimeOpenConfiguration(
                        developmentMode = true,
                        explicitDataDirectory = it.toString(),
                    ),
                ),
            )
        },
    standardOutput: (String) -> Unit = ::println,
    errorOutput: (String) -> Unit = System.err::println,
): Int {
    var ownedRoot: OwnedHealthDataRoot? = null
    var runtime: PackagedHealthRuntime? = null
    var closed = false
    var stage = "ROOT"
    var failureEvidence: String? = null
    try {
        withTimeout(timeoutMillis) {
            ownedRoot = claimHealthDataRoot(developmentDataDirectory)
            stage = "COMPATIBILITY"
            verifyNativeCompatibility(compatibilityDescriptor())
            stage = "OPEN"
            runtime = runtimeOpener.open(requireNotNull(ownedRoot).path)
            stage = "BOOTSTRAP"
            val lifecycle = requireNotNull(runtime).bootstrapLifecycle()
            stage = "READY"
            require(lifecycle in setOf(ApplicationLifecycle.Ready, ApplicationLifecycle.Degraded))
            standardOutput(HEALTH_READY_EVIDENCE)
            stage = "SHUTDOWN"
            require(requireNotNull(runtime).shutdownClosed())
            closed = true
            standardOutput(HEALTH_CLOSED_EVIDENCE)
        }
    } catch (_: kotlinx.coroutines.TimeoutCancellationException) {
        failureEvidence = "$HEALTH_FAILURE_EVIDENCE:TIMEOUT"
    } catch (error: HarvestCircleException.Failure) {
        failureEvidence = "$HEALTH_FAILURE_EVIDENCE:$stage:${error.code}:${error.safeMessage}"
    } catch (error: Exception) {
        failureEvidence = "$HEALTH_FAILURE_EVIDENCE:$stage:${error.javaClass.simpleName}"
    } finally {
        var safeToCleanup = runtime == null || closed
        if (!closed && runtime != null) {
            val shutdown = runCatching { withTimeout(timeoutMillis) { requireNotNull(runtime).shutdownClosed() } }
            if (shutdown.getOrDefault(false)) {
                closed = true
                safeToCleanup = true
            } else if (failureEvidence == null) {
                failureEvidence = "$HEALTH_FAILURE_EVIDENCE:SHUTDOWN:IllegalStateException"
            }
        }
        ownedRoot?.takeIf { safeToCleanup }?.let { root ->
            runCatching { root.cleanup() }
                .onFailure {
                    if (failureEvidence == null) {
                        failureEvidence = "$HEALTH_FAILURE_EVIDENCE:CLEANUP:${it.javaClass.simpleName}"
                    }
                }
        }
    }
    failureEvidence?.let(errorOutput)
    return if (failureEvidence == null) 0 else 1
}

internal class OwnedHealthDataRoot internal constructor(
    val path: Path,
    private val marker: Path,
    private val ownershipToken: String,
    private val createdByHealthCheck: Boolean,
) {
    fun cleanup() {
        require(Files.exists(path, LinkOption.NOFOLLOW_LINKS) && Files.isDirectory(path, LinkOption.NOFOLLOW_LINKS))
        requireNoSymbolicLinks(path)
        require(path.toRealPath() == path)
        require(Files.isRegularFile(marker, LinkOption.NOFOLLOW_LINKS) && !Files.isSymbolicLink(marker))
        require(Files.readString(marker) == ownershipToken)

        Files.walkFileTree(
            path,
            object : SimpleFileVisitor<Path>() {
                override fun visitFile(
                    file: Path,
                    attributes: BasicFileAttributes,
                ): FileVisitResult {
                    Files.delete(file)
                    return FileVisitResult.CONTINUE
                }

                override fun postVisitDirectory(
                    directory: Path,
                    error: java.io.IOException?,
                ): FileVisitResult {
                    if (error != null) throw error
                    if (directory != path || createdByHealthCheck) Files.delete(directory)
                    return FileVisitResult.CONTINUE
                }
            },
        )
    }
}

internal fun claimHealthDataRoot(rawPath: String?): OwnedHealthDataRoot {
    require(!rawPath.isNullOrBlank()) { "A dedicated health data root is required" }
    val requested = Path.of(rawPath).normalize()
    require(requested.isAbsolute) { "The health data root must be absolute" }
    val parent = requireNotNull(requested.parent) { "The health data root must have a parent" }
    requireNoSymbolicLinks(parent)
    require(Files.isDirectory(parent, LinkOption.NOFOLLOW_LINKS)) { "The health data parent must exist" }

    val created = !Files.exists(requested, LinkOption.NOFOLLOW_LINKS)
    if (created) {
        Files.createDirectory(requested)
    } else {
        require(!Files.isSymbolicLink(requested)) { "The health data root cannot be a symbolic link" }
        require(Files.isDirectory(requested, LinkOption.NOFOLLOW_LINKS)) { "The health data root must be a directory" }
        Files.list(requested).use { entries -> require(entries.findAny().isEmpty) { "The health data root must be empty" } }
    }

    return try {
        requireNoSymbolicLinks(requested)
        val canonical = requested.toRealPath()
        val token =
            ByteArray(HEALTH_OWNER_TOKEN_BYTES)
                .also(healthOwnershipRandom::nextBytes)
                .joinToString(separator = "") { byte -> "%02x".format(byte.toInt() and 0xff) }
        val marker = canonical.resolve(HEALTH_OWNER_MARKER)
        Files.writeString(marker, token, StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE)
        OwnedHealthDataRoot(canonical, marker, token, created)
    } catch (error: Exception) {
        if (created) runCatching { Files.deleteIfExists(requested) }
        throw error
    }
}

private fun requireNoSymbolicLinks(path: Path) {
    var current = requireNotNull(path.root) { "The health data root must be absolute" }
    path.forEach { segment ->
        current = current.resolve(segment)
        require(!Files.isSymbolicLink(current)) { "The health data root cannot traverse a symbolic link" }
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
