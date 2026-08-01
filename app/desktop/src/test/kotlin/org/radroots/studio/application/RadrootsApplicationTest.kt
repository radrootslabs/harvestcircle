package org.radroots.studio.application

import androidx.compose.foundation.text.BasicText
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.v2.runComposeUiTest
import kotlin.test.Test

class RadrootsApplicationTest {
    @OptIn(ExperimentalTestApi::class)
    @Test
    fun composeUiTestRuntimeRendersContent() = runComposeUiTest {
        setContent {
            BasicText("radroots")
        }

        onNodeWithText("radroots").assertIsDisplayed()
    }
}
