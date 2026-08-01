package org.radroots.studio.desktop

import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import java.awt.Dimension
import java.awt.Taskbar
import javax.imageio.ImageIO
import org.radroots.studio.application.RadrootsApplication

private const val ApplicationName = "Radroots"
private const val InitialWindowWidth = 1284
private const val InitialWindowHeight = 795
private const val MinimumWindowWidth = 1080
private const val MinimumWindowHeight = 720

private val isMacOs: Boolean =
    System.getProperty("os.name", "")
        .startsWith("Mac", ignoreCase = true)

fun main() {
    if (isMacOs) {
        configureMacOsApplication()
    }

    application {
        Window(
            onCloseRequest = ::exitApplication,
            title = ApplicationName,
            state = rememberWindowState(
                width = InitialWindowWidth.dp,
                height = InitialWindowHeight.dp,
            ),
        ) {
            DisposableEffect(window) {
                window.minimumSize = Dimension(MinimumWindowWidth, MinimumWindowHeight)

                if (isMacOs) {
                    val rootPane = window.rootPane
                    rootPane.putClientProperty("apple.awt.fullWindowContent", true)
                    rootPane.putClientProperty("apple.awt.transparentTitleBar", true)
                    rootPane.putClientProperty("apple.awt.windowTitleVisible", false)
                }

                onDispose { }
            }

            RadrootsApplication()
        }
    }
}

private fun configureMacOsApplication() {
    System.setProperty("apple.awt.application.name", ApplicationName)
    System.setProperty("apple.awt.application.appearance", "system")

    if (!Taskbar.isTaskbarSupported()) return

    val taskbar = Taskbar.getTaskbar()
    if (!taskbar.isSupported(Taskbar.Feature.ICON_IMAGE)) return

    val resource = checkNotNull(
        Thread.currentThread().contextClassLoader.getResource("icons/radroots.png"),
    ) {
        "Missing runtime app icon"
    }
    taskbar.iconImage = checkNotNull(ImageIO.read(resource)) {
        "Invalid runtime app icon"
    }
}
