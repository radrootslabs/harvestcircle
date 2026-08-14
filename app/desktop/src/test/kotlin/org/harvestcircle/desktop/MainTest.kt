package org.harvestcircle.desktop

import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeEdge
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeExclusion
import java.io.ByteArrayInputStream
import java.nio.file.Files
import javax.swing.JRootPane
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class MainTest {
    @Test
    fun desktopWindowUsesTheFoundationDimensions() {
        assertEquals(1280, INITIAL_WINDOW_WIDTH)
        assertEquals(800, INITIAL_WINDOW_HEIGHT)
        assertEquals(1100, MINIMUM_WINDOW_WIDTH)
        assertEquals(720, MINIMUM_WINDOW_HEIGHT)
    }

    @Test
    fun macOsWindowChromeHidesTheVisualTitleButKeepsNativeControlsAndAccessibility() {
        val rootPane = JRootPane()

        configureMacOsWindowChrome(rootPane, rootPane.accessibleContext)

        assertEquals("", desktopWindowTitle(macOs = true))
        assertEquals("HarvestCircle", desktopWindowTitle(macOs = false))
        assertEquals(true, rootPane.getClientProperty("apple.awt.fullWindowContent"))
        assertEquals(true, rootPane.getClientProperty("apple.awt.transparentTitleBar"))
        assertEquals(true, rootPane.getClientProperty("apple.awt.windowTitleVisible"))
        assertEquals("HarvestCircle", rootPane.accessibleContext.accessibleName)
    }

    @Test
    fun macOsWindowChromeReservesAConservativePhysicalExclusion() {
        assertEquals(
            HarvestCircleWindowChromeExclusion(
                edge = HarvestCircleWindowChromeEdge.Left,
                width = 112.dp,
                height = 40.dp,
            ),
            desktopWindowChromeExclusion(macOs = true),
        )
        assertEquals(HarvestCircleWindowChromeExclusion.None, desktopWindowChromeExclusion(macOs = false))
    }

    @Test
    fun missingOrInvalidRuntimeIconFailsSafely() {
        assertNull(loadRuntimeIcon { null })
        assertNull(loadRuntimeIcon { ByteArrayInputStream("not an image".encodeToByteArray()) })
    }

    @Test
    fun healthCheckEntryRequiresTheSingleSupportedArgument() {
        assertTrue(isHealthCheck(arrayOf(HEALTH_CHECK_ARGUMENT)))
        assertFalse(isHealthCheck(emptyArray()))
        assertFalse(isHealthCheck(arrayOf(HEALTH_CHECK_ARGUMENT, "unexpected")))
    }

    @Test
    fun invalidHealthRootsFailBeforeTheRuntimeOpener() =
        runBlocking {
            val parent = Files.createTempDirectory("harvestcircle-health-invalid-").toRealPath()
            val nonempty = Files.createDirectory(parent.resolve("nonempty"))
            Files.writeString(nonempty.resolve("foreign"), "owned elsewhere")
            val file = Files.writeString(parent.resolve("file"), "not a directory")
            val target = Files.createDirectory(parent.resolve("target"))
            var openCalls = 0
            val opener =
                PackagedHealthRuntimeOpener {
                    openCalls += 1
                    successfulRuntime()
                }

            val invalidRoots =
                mutableListOf<String?>(
                    null,
                    "",
                    "relative/path",
                    parent.resolve("missing-parent/root").toString(),
                    nonempty.toString(),
                    file.toString(),
                )
            runCatching { Files.createSymbolicLink(parent.resolve("link"), target) }
                .getOrNull()
                ?.let { invalidRoots += it.toString() }

            invalidRoots.forEach { invalid ->
                assertEquals(1, executeHealthCheck(invalid, runtimeOpener = opener, errorOutput = {}))
            }
            assertEquals(0, openCalls)
        }

    @Test
    fun healthCheckOwnsBootstrapsShutsDownAndCleansCreatedAndProvidedRoots() =
        runBlocking {
            val parent = Files.createTempDirectory("harvestcircle-health-valid-").toRealPath()
            val createdRoot = parent.resolve("created")
            val providedRoot = Files.createDirectory(parent.resolve("provided"))
            val output = mutableListOf<String>()
            var openCalls = 0
            val openedRoots = mutableListOf<java.nio.file.Path>()
            val expectedRoots = listOf(createdRoot.toAbsolutePath().normalize(), providedRoot.toRealPath())

            for (root in listOf(createdRoot, providedRoot)) {
                val result =
                    executeHealthCheck(
                        root.toString(),
                        runtimeOpener =
                            PackagedHealthRuntimeOpener { ownedRoot ->
                                openCalls += 1
                                openedRoots.add(ownedRoot)
                                Files.writeString(ownedRoot.resolve("runtime.sqlite3"), "runtime data")
                                successfulRuntime()
                            },
                        standardOutput = output::add,
                        errorOutput = output::add,
                    )
                assertEquals(0, result)
            }

            assertEquals(2, openCalls)
            assertEquals(expectedRoots, openedRoots)
            assertFalse(Files.exists(createdRoot))
            assertTrue(Files.isDirectory(providedRoot))
            Files.list(providedRoot).use { entries -> assertTrue(entries.findAny().isEmpty) }
            assertEquals(
                listOf(HEALTH_READY_EVIDENCE, HEALTH_CLOSED_EVIDENCE, HEALTH_READY_EVIDENCE, HEALTH_CLOSED_EVIDENCE),
                output,
            )
        }

    @Test
    fun cleanupRefusesMissingOrChangedOwnershipMarkersWithoutDeletingData() {
        val parent = Files.createTempDirectory("harvestcircle-health-marker-").toRealPath()
        listOf("missing", "changed").forEach { caseName ->
            val root = Files.createDirectory(parent.resolve(caseName))
            val ownership = claimHealthDataRoot(root.toString())
            val marker = Files.list(root).use { entries -> entries.findFirst().orElseThrow() }
            val retained = Files.writeString(root.resolve("retained"), "caller data")
            if (caseName == "missing") Files.delete(marker) else Files.writeString(marker, "different owner")

            assertFailsWith<IllegalArgumentException> { ownership.cleanup() }
            assertTrue(Files.exists(retained))
        }
    }

    @Test
    fun timeoutAndFailureEvidenceDoNotExposeExceptionSecrets() =
        runBlocking {
            val parent = Files.createTempDirectory("harvestcircle-health-redaction-").toRealPath()
            val secret = "nsec1must-not-escape"
            val evidence = mutableListOf<String>()
            val timeoutRuntime =
                object : PackagedHealthRuntime {
                    override suspend fun bootstrapLifecycle(): ApplicationLifecycle {
                        delay(50)
                        return ApplicationLifecycle.Ready
                    }

                    override suspend fun shutdownClosed(): Boolean = true
                }

            assertEquals(
                1,
                executeHealthCheck(
                    parent.resolve("timeout").toString(),
                    timeoutMillis = 1,
                    runtimeOpener = PackagedHealthRuntimeOpener { timeoutRuntime },
                    errorOutput = evidence::add,
                ),
            )
            assertEquals(
                1,
                executeHealthCheck(
                    parent.resolve("failure").toString(),
                    runtimeOpener = PackagedHealthRuntimeOpener { throw IllegalStateException(secret) },
                    errorOutput = evidence::add,
                ),
            )
            assertTrue(evidence.any { it == "HARVESTCIRCLE_HEALTH_FAILED:TIMEOUT" })
            assertTrue(evidence.any { it.contains(":OPEN:IllegalStateException") })
            assertFalse(evidence.joinToString().contains(secret))
            assertFalse(Files.exists(parent.resolve("timeout")))
            assertFalse(Files.exists(parent.resolve("failure")))
        }

    @Test
    fun failedShutdownRetainsTheOwnedRootInsteadOfDeletingLiveRuntimeData() =
        runBlocking {
            val parent = Files.createTempDirectory("harvestcircle-health-shutdown-").toRealPath()
            val root = parent.resolve("retained")
            val runtime =
                object : PackagedHealthRuntime {
                    override suspend fun bootstrapLifecycle(): ApplicationLifecycle = ApplicationLifecycle.Ready

                    override suspend fun shutdownClosed(): Boolean = false
                }

            assertEquals(
                1,
                executeHealthCheck(
                    root.toString(),
                    runtimeOpener = PackagedHealthRuntimeOpener { runtime },
                    errorOutput = {},
                ),
            )
            assertTrue(Files.isDirectory(root))
            Files.list(root).use { entries -> assertTrue(entries.findAny().isPresent) }
        }
}

private fun successfulRuntime(): PackagedHealthRuntime =
    object : PackagedHealthRuntime {
        override suspend fun bootstrapLifecycle(): ApplicationLifecycle = ApplicationLifecycle.Ready

        override suspend fun shutdownClosed(): Boolean = true
    }
