package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import androidx.compose.ui.unit.dp
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.ThemePreference
import org.harvestcircle.application.ApplicationLifecycle
import org.harvestcircle.application.ApplicationSnapshot
import org.harvestcircle.application.BuildInfo
import org.harvestcircle.application.HarvestCirclePresenterState
import org.harvestcircle.application.HarvestCircleShellState
import org.harvestcircle.application.SessionLifecycle
import org.harvestcircle.application.ShellSessionState
import org.harvestcircle.application.SnapshotRevision
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import java.awt.image.BufferedImage
import java.io.File
import javax.imageio.ImageIO
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertNotNull

@OptIn(ExperimentalTestApi::class)
class HarvestCircleMacGoldenTest {
    @Test
    fun macosAarch64LiveTodayShellMatches() {
        val updating = System.getProperty("harvestcircle.updateMacosGoldens") == "true"
        if (!isMacosAarch64()) {
            check(!updating) { "HarvestCircle design goldens may only be updated on macos-aarch64" }
            return
        }

        listOf(ThemePreference.Light, ThemePreference.Dark).forEach { theme ->
            val actual = captureLiveTodayShell(theme)
            val resourceName = "goldens/macos-aarch64/design-surface-${theme.name.lowercase()}.png"
            if (updating) {
                val projectDir = assertNotNull(System.getProperty("harvestcircle.projectDir"))
                val output = File(projectDir, "app/shared/src/desktopTest/resources/$resourceName")
                output.parentFile.mkdirs()
                check(ImageIO.write(actual, "png", output))
            } else {
                val stream =
                    assertNotNull(
                        javaClass.classLoader.getResourceAsStream(resourceName),
                        "Missing golden resource: $resourceName",
                    )
                val expected = assertNotNull(stream.use { ImageIO.read(it) }, "Unreadable golden resource: $resourceName")
                assertEquals(expected.width, actual.width, "$resourceName width")
                assertEquals(expected.height, actual.height, "$resourceName height")
                assertContentEquals(expected.argb(), actual.argb(), resourceName)
            }
        }
    }
}

@OptIn(ExperimentalTestApi::class)
private fun captureLiveTodayShell(theme: ThemePreference): BufferedImage {
    lateinit var captured: ImageBitmap
    runComposeUiTest {
        setContent {
            Box(Modifier.size(width = 1280.dp, height = 800.dp).testTag("golden-surface")) {
                HarvestCircleShell(
                    state = liveTodayState(theme),
                    identityActions = HarvestCircleUiActions(),
                    platformActions = HarvestCirclePlatformActions(),
                    dispatch = {},
                )
            }
        }
        waitForIdle()
        captured = onNodeWithTag("golden-surface").captureToImage()
    }
    return captured.toBufferedImage()
}

private fun liveTodayState(theme: ThemePreference): HarvestCircleShellState =
    HarvestCircleShellState(
        identity =
            HarvestCirclePresenterState(
                ApplicationSnapshot(
                    revision = SnapshotRevision(1UL),
                    lifecycle = ApplicationLifecycle.Ready,
                    lifecycleProblem = null,
                    configuredRelays = emptyList(),
                    identities = emptyList(),
                    selectedIdentityId = null,
                    session = SessionLifecycle.SignedOut,
                    sessionSubjectIdentityId = null,
                    sessionProblem = null,
                    activeIdentity = null,
                    recoverableProblem = null,
                ),
            ),
        buildInfo = BuildInfo.unknown(),
        session = ShellSessionState(readOnly = true),
        appearance = AppearanceState(theme = theme),
    )

private fun ImageBitmap.toBufferedImage(): BufferedImage {
    val pixels = toPixelMap()
    return BufferedImage(width, height, BufferedImage.TYPE_INT_ARGB).also { image ->
        for (y in 0 until height) {
            for (x in 0 until width) {
                image.setRGB(x, y, pixels[x, y].toArgb())
            }
        }
    }
}

private fun BufferedImage.argb(): IntArray = getRGB(0, 0, width, height, null, 0, width)

private fun isMacosAarch64(): Boolean =
    System.getProperty("os.name").equals("Mac OS X", ignoreCase = true) &&
        System.getProperty("os.arch").lowercase() in setOf("aarch64", "arm64")
