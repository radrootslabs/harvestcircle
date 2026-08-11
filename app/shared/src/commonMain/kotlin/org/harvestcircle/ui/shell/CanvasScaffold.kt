package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.harvestcircle.design.TextSizePreference

@Composable
fun CanvasScaffold(
    textSize: TextSizePreference,
    navigation: @Composable () -> Unit = {},
    header: @Composable () -> Unit,
    step: @Composable () -> Unit = {},
    body: @Composable () -> Unit,
    actionBar: @Composable () -> Unit,
) {
    Column(Modifier.fillMaxSize().testTag("canvas-scaffold")) {
        Row(
            Modifier
                .fillMaxWidth()
                .height(64.dp)
                .semantics { contentDescription = "Canvas header" }
                .testTag("canvas-header"),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.testTag("canvas-navigation")) { navigation() }
            Box(Modifier.weight(1f)) { header() }
            Box(Modifier.testTag("canvas-step")) { step() }
        }
        val bodyModifier =
            Modifier
                .weight(1f)
                .fillMaxWidth()
                .let { modifier ->
                    if (canvasBodyScroll(textSize) == ScrollOwnership.CanvasBodyAccessibility) {
                        modifier.verticalScroll(rememberScrollState())
                    } else {
                        modifier
                    }
                }.semantics { contentDescription = "Canvas body" }
                .testTag("canvas-body")
        Box(bodyModifier, contentAlignment = Alignment.TopCenter) {
            Box(Modifier.fillMaxWidth().widthIn(max = 960.dp)) { body() }
        }
        Box(
            Modifier
                .fillMaxWidth()
                .height(72.dp)
                .semantics { contentDescription = "Canvas action bar" }
                .testTag("canvas-action-bar"),
        ) {
            actionBar()
        }
    }
}
