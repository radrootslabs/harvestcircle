package org.harvestcircle.application

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeEnvironment
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeExclusion
import java.io.File
import java.net.URLClassLoader
import java.nio.file.Files
import java.util.concurrent.TimeUnit
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class DefaultProfileStartupTest {
    @Test
    fun realApplicationOpensAndClosesTwiceInAFreshDefaultProfile() {
        val home = Files.createTempDirectory("harvestcircle-default-profile-").toRealPath()
        try {
            val inherited = System.getProperty("java.class.path").split(File.pathSeparator)
            val urls =
                generateSequence(javaClass.classLoader) { it.parent }
                    .filterIsInstance<URLClassLoader>()
                    .flatMap { it.urLs.asSequence() }
                    .map { File(it.toURI()).absolutePath }
                    .toList()
            val classpath = (inherited + urls).distinct().joinToString(File.pathSeparator)
            assertTrue(classpath.isNotEmpty())
            repeat(3) { iteration ->
                val profile = if (iteration == 2) Files.createDirectory(home.resolve("missing-profile")) else home
                val library = if (iteration == 2) profile.resolve("unavailable/harvestcircle_ffi").toString() else "harvestcircle_ffi"
                val output = home.resolve("probe-$iteration.log").toFile()
                val builder =
                    ProcessBuilder(
                        File(System.getProperty("java.home"), "bin/java").absolutePath,
                        "-Duser.home=$profile",
                        "-Duniffi.component.harvestcircle_ffi.libraryOverride=$library",
                        "-Djava.io.tmpdir=${System.getProperty("java.io.tmpdir")}",
                        "-Djna.library.path=${System.getProperty("jna.library.path")}",
                        "-cp",
                        classpath,
                        "org.harvestcircle.application.DefaultProfileStartupProbe",
                        if (iteration == 2) "missing" else "normal",
                    ).redirectErrorStream(true).redirectOutput(output)
                builder.environment().apply {
                    put("HOME", profile.toString())
                    remove("HARVESTCIRCLE_DEVELOPMENT_DATA_DIR")
                    remove("HARVESTCIRCLE_NOSTR_RELAYS")
                    remove("JAVA_TOOL_OPTIONS")
                    remove("JDK_JAVA_OPTIONS")
                    remove("_JAVA_OPTIONS")
                    remove("XDG_DATA_HOME")
                    remove("XDG_STATE_HOME")
                    remove("XDG_CONFIG_HOME")
                    remove("XDG_CACHE_HOME")
                    val runtime = profile.resolve("run").toFile()
                    runtime.mkdir()
                    put("XDG_RUNTIME_DIR", runtime.absolutePath)
                }
                val process = builder.start()
                try {
                    assertTrue(process.waitFor(120, TimeUnit.SECONDS), "Default profile probe timed out")
                    assertEquals(0, process.exitValue(), output.readText())
                    assertTrue(output.readText().contains("HARVESTCIRCLE_DEFAULT_PROFILE_CLOSED"))
                    if (iteration == 2) {
                        assertTrue(output.readText().contains("code=NativeLibraryLoadFailed"))
                        assertTrue(Files.notExists(profile.resolve("Library/Application Support/Radroots/data")))
                        assertTrue(Files.notExists(profile.resolve(".local/share/radroots")))
                    }
                } finally {
                    if (process.isAlive) process.destroyForcibly().waitFor()
                }
            }
        } finally {
            home.toFile().deleteRecursively()
        }
    }
}

internal object DefaultProfileStartupProbe {
    @JvmStatic
    @OptIn(ExperimentalTestApi::class)
    fun main(args: Array<String>) {
        runComposeUiTest {
            var closeRequested by mutableStateOf(false)
            var closed = false
            setContent {
                HarvestCircleWindowChromeEnvironment(HarvestCircleWindowChromeExclusion.None) {
                    HarvestCircleApplication(closeRequested = closeRequested, onExitApproved = { closed = true })
                }
            }
            val expectedTag = if (args.single() == "missing") "startup-failure" else "bootstrap-read-only"
            waitUntil(timeoutMillis = 30_000) {
                onAllNodesWithTag(expectedTag).fetchSemanticsNodes().isNotEmpty() ||
                    onAllNodesWithTag("startup-failure").fetchSemanticsNodes().isNotEmpty()
            }
            assertTrue(onAllNodesWithTag(expectedTag).fetchSemanticsNodes().isNotEmpty(), "Unexpected startup failure")
            if (args.single() == "missing") {
                onAllNodesWithText("Try again").assertCountEquals(0)
            }
            runOnUiThread { closeRequested = true }
            waitUntil(timeoutMillis = 30_000) { closed }
        }
        println("HARVESTCIRCLE_DEFAULT_PROFILE_CLOSED")
    }
}
