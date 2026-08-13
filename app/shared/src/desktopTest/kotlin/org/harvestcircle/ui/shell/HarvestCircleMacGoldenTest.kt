package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
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
import org.harvestcircle.designsystem.component.HarvestCircleButtonVariant
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.action.HarvestCircleLabeledButton
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBadge
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBanner
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBannerTone
import org.harvestcircle.designsystem.component.navigation.HarvestCircleNavigationItem
import org.harvestcircle.designsystem.layout.HarvestCirclePane
import org.harvestcircle.designsystem.layout.HarvestCircleSidebar
import org.harvestcircle.designsystem.layout.HarvestCircleSidebarSectionHeader
import org.harvestcircle.designsystem.layout.HarvestCircleToolbar
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleDesignTheme
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.harvestcircle.designsystem.theme.HarvestCircleThemeConfig
import org.harvestcircle.designsystem.theme.HarvestCircleThemeMode
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
    fun macosAarch64LightAndDarkReferenceSurfacesMatch() {
        val updating = System.getProperty("harvestcircle.updateMacosGoldens") == "true"
        if (!isMacosAarch64()) {
            check(!updating) { "HarvestCircle design goldens may only be updated on macos-aarch64" }
            return
        }

        listOf(HarvestCircleThemeMode.Light, HarvestCircleThemeMode.Dark).forEach { mode ->
            val actual = captureReferenceSurface(mode)
            val resourceName = "goldens/macos-aarch64/design-surface-${mode.name.lowercase()}.png"
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
private fun captureReferenceSurface(mode: HarvestCircleThemeMode): BufferedImage {
    lateinit var captured: ImageBitmap
    runComposeUiTest {
        setContent {
            HarvestCircleDesignTheme(HarvestCircleThemeConfig(mode = mode)) {
                Row(Modifier.size(width = 760.dp, height = 420.dp).testTag("golden-surface")) {
                    HarvestCircleSidebar(Modifier.width(180.dp)) {
                        HarvestCircleSidebarSectionHeader("HarvestCircle")
                        HarvestCircleNavigationItem(true, {}, "Today")
                        HarvestCircleNavigationItem(false, {}, "Network")
                        HarvestCircleNavigationItem(false, {}, "Settings")
                        Spacer(Modifier.weight(1f))
                        HarvestCircleText(
                            "Local-first · Nostr",
                            modifier = Modifier.padding(HarvestCircleTheme.foundation.spacing.md),
                            role = HarvestCircleTextRole.LabelSmall,
                            tone = HarvestCircleContentTone.Muted,
                        )
                    }
                    Column(Modifier.weight(1f).fillMaxHeight()) {
                        HarvestCircleToolbar {
                            HarvestCircleText("Today", role = HarvestCircleTextRole.SectionTitle)
                            Spacer(Modifier.weight(1f))
                            HarvestCircleBadge("Signed out")
                        }
                        HarvestCirclePane(
                            modifier = Modifier.fillMaxSize(),
                            role = org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole.Canvas,
                        ) {
                            Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap)) {
                                HarvestCircleText("Coordinate local food", role = HarvestCircleTextRole.PageTitle)
                                HarvestCircleText("Clear, signed terms for farms and nearby buyers.")
                                HarvestCircleBanner(
                                    message = "No managed HarvestCircle service is configured.",
                                    tone = HarvestCircleBannerTone.Info,
                                    title = "Local runtime",
                                )
                                Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                                    HarvestCircleLabeledButton("Open a Nostr reference", "Open a Nostr reference", {})
                                    HarvestCircleLabeledButton(
                                        "Explore circles",
                                        "Explore circles",
                                        {},
                                        enabled = false,
                                        variant = HarvestCircleButtonVariant.Secondary,
                                    )
                                }
                                HarvestCircleText(
                                    "Not available in this build.",
                                    role = HarvestCircleTextRole.BodySmall,
                                    tone = HarvestCircleContentTone.Secondary,
                                )
                            }
                        }
                    }
                }
            }
        }
        waitForIdle()
        captured = onNodeWithTag("golden-surface").captureToImage()
    }
    return captured.toBufferedImage()
}

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
