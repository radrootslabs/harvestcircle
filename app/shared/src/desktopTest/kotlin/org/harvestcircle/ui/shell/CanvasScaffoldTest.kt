package org.harvestcircle.ui.shell

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.v2.runComposeUiTest
import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.TextSizePreference
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@OptIn(ExperimentalTestApi::class)
class CanvasScaffoldTest {
    @Test
    fun baselineCanvasKeepsHeaderBodyAndActionRegionsFixed() =
        runComposeUiTest {
            setHarvestCircleContent { canvas(TextSizePreference.Default) }
            onNodeWithTag("canvas-header").assertIsDisplayed()
            onNodeWithTag("canvas-body").assertIsDisplayed()
            onNodeWithTag("canvas-action-bar").assertIsDisplayed()
            onAllNodesWithTag("canvas-navigation").assertCountEquals(1)
            assertFalse(onNodeWithTag("canvas-body").fetchSemanticsNode().config.contains(SemanticsActions.ScrollBy))
        }

    @Test
    fun veryLargeTextEnablesOnlyBoundedBodyScrolling() =
        runComposeUiTest {
            setHarvestCircleContent { canvas(TextSizePreference.VeryLarge) }
            assertTrue(onNodeWithTag("canvas-body").fetchSemanticsNode().config.contains(SemanticsActions.ScrollBy))
            onNodeWithTag("canvas-action-bar").assertIsDisplayed()
        }

    @Test
    fun shellAppearanceEnablesLargeTextFallbackForDefaultCanvases() =
        runComposeUiTest {
            setHarvestCircleContent {
                CompositionLocalProvider(
                    LocalShellAppearance provides AppearanceState(textSize = TextSizePreference.VeryLarge),
                ) {
                    canvas(TextSizePreference.Default)
                }
            }
            assertTrue(onNodeWithTag("canvas-body").fetchSemanticsNode().config.contains(SemanticsActions.ScrollBy))
        }
}

@androidx.compose.runtime.Composable
private fun canvas(textSize: TextSizePreference) {
    CanvasScaffold(
        textSize = textSize,
        navigation = {},
        header = {},
        step = {},
        body = {},
        actionBar = {},
    )
}
