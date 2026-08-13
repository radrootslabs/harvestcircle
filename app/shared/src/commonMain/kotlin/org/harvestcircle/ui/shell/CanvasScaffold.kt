package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.designsystem.layout.HarvestCircleCanvasFrame

@Composable
fun CanvasScaffold(
    textSize: TextSizePreference,
    navigation: @Composable () -> Unit = {},
    header: @Composable () -> Unit,
    step: @Composable () -> Unit = {},
    body: @Composable () -> Unit,
    actionBar: @Composable () -> Unit,
) {
    val effectiveTextSize =
        textSize.takeUnless { it == TextSizePreference.Default }
            ?: LocalShellAppearance.current.textSize
    HarvestCircleCanvasFrame(
        modifier = Modifier.fillMaxSize().testTag("canvas-scaffold"),
        navigation = {
            Box(Modifier.testTag("canvas-navigation")) { navigation() }
        },
        header = {
            Box(
                Modifier
                    .fillMaxSize()
                    .semantics { contentDescription = "Canvas header" }
                    .testTag("canvas-header"),
            ) {
                header()
            }
        },
        step = {
            Box(Modifier.testTag("canvas-step")) { step() }
        },
        bodyScrollable = canvasBodyScroll(effectiveTextSize) == ScrollOwnership.CanvasBodyAccessibility,
        bodyModifier =
            Modifier
                .semantics { contentDescription = "Canvas body" }
                .testTag("canvas-body"),
        body = {
            body()
        },
        actionBar = {
            Box(
                Modifier
                    .fillMaxSize()
                    .semantics { contentDescription = "Canvas action bar" }
                    .testTag("canvas-action-bar"),
            ) {
                actionBar()
            }
        },
    )
}
