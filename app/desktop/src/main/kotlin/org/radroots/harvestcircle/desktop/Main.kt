package org.radroots.harvestcircle.desktop

import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import org.radroots.harvestcircle.accounts.ui.StartupFailureScreen
import org.radroots.harvestcircle.application.HarvestCircleApplication
import java.awt.Dimension
import java.awt.Taskbar
import javax.imageio.ImageIO

private const val APPLICATION_NAME = "HarvestCircle"
private const val INITIAL_WINDOW_WIDTH = 1284
private const val INITIAL_WINDOW_HEIGHT = 795
private const val MINIMUM_WINDOW_WIDTH = 1080
private const val MINIMUM_WINDOW_HEIGHT = 720

private val isMacOs: Boolean =
    System
        .getProperty("os.name", "")
        .startsWith("Mac", ignoreCase = true)

fun main() {
    val nativeStartupProblem = if (isMacOs) configureMacOsApplication() else null

    application {
        Window(
            onCloseRequest = ::exitApplication,
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
                HarvestCircleApplication()
            } else {
                StartupFailureScreen(nativeStartupProblem)
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
