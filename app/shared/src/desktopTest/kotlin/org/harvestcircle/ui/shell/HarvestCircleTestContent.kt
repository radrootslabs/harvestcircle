package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.test.ComposeUiTest
import androidx.compose.ui.test.ExperimentalTestApi
import org.harvestcircle.design.AppearanceState

@OptIn(ExperimentalTestApi::class)
internal fun ComposeUiTest.setHarvestCircleContent(content: @Composable () -> Unit) {
    setContent {
        HarvestCircleTheme(
            appearance = AppearanceState(),
            systemDark = false,
            content = content,
        )
    }
}
