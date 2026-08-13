package org.harvestcircle.designcatalog

import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState

public fun main(): Unit =
    application {
        Window(
            onCloseRequest = ::exitApplication,
            title = "HarvestCircle UI Catalog",
            state =
                rememberWindowState(
                    width = 1180.dp,
                    height = 820.dp,
                ),
        ) {
            HarvestCircleCatalogApp()
        }
    }
