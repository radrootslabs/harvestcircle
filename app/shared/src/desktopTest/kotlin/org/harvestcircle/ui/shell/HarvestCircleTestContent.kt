package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.test.ComposeUiTest
import androidx.compose.ui.test.ExperimentalTestApi
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeEnvironment
import org.harvestcircle.designsystem.layout.HarvestCircleWindowChromeExclusion

@OptIn(ExperimentalTestApi::class)
internal fun ComposeUiTest.setHarvestCircleContent(
    windowChromeExclusion: HarvestCircleWindowChromeExclusion = HarvestCircleWindowChromeExclusion.None,
    content: @Composable () -> Unit,
) {
    setContent {
        HarvestCircleWindowChromeEnvironment(windowChromeExclusion) {
            HarvestCircleTheme(
                appearance = AppearanceState(),
                systemDark = false,
                content = content,
            )
        }
    }
}
