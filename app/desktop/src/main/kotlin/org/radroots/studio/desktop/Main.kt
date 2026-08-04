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
import org.radroots.studio.accounts.ui.StartupFailureScreen

private const val ApplicationName = "Radroots"
private const val InitialWindowWidth = 1284
private const val InitialWindowHeight = 795
private const val MinimumWindowWidth = 1080
private const val MinimumWindowHeight = 720

private val isMacOs: Boolean =
    System.getProperty("os.name", "")
        .startsWith("Mac", ignoreCase = true)

fun main() {
    val nativeStartupProblem = if (isMacOs) configureMacOsApplication() else null

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

            if (nativeStartupProblem == null) {
                RadrootsApplication()
            } else {
                StartupFailureScreen(nativeStartupProblem)
            }
        }
    }
}

private fun configureMacOsApplication(): String? {
    System.setProperty("apple.awt.application.name", ApplicationName)
    System.setProperty("apple.awt.application.appearance", "system")

    if (!Taskbar.isTaskbarSupported()) return null

    val taskbar = Taskbar.getTaskbar()
    if (!taskbar.isSupported(Taskbar.Feature.ICON_IMAGE)) return null

    val icon = loadRuntimeIcon {
        Thread.currentThread().contextClassLoader.getResourceAsStream("icons/radroots.png")
    } ?: return "The application icon resource is unavailable."
    return runCatching { taskbar.iconImage = icon }
        .fold(
            onSuccess = { null },
            onFailure = { "The application icon could not be configured." },
        )
}

internal fun loadRuntimeIcon(openResource: () -> java.io.InputStream?): java.awt.Image? =
    runCatching { openResource()?.use(ImageIO::read) }.getOrNull()
