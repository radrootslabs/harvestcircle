package org.harvestcircle.designsystem.layout

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.RectangleShape
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/**
 * Generic editor, inspector, or navigation pane.
 *
 * macOS application panes are rectangular by default; [rounded] is reserved for nested panels.
 */
@Composable
public fun HarvestCirclePane(
    modifier: Modifier = Modifier,
    role: HarvestCircleSurfaceRole = HarvestCircleSurfaceRole.Base,
    padded: Boolean = true,
    outlined: Boolean = false,
    rounded: Boolean = false,
    content: @Composable ColumnScope.() -> Unit,
) {
    HarvestCircleSurface(
        modifier = modifier,
        role = role,
        shape = if (rounded) HarvestCircleTheme.foundation.shapes.panel else RectangleShape,
        border =
            if (outlined) {
                BorderStroke(
                    width = HarvestCircleTheme.shell.dimensions.dividerWidth,
                    color = HarvestCircleTheme.foundation.colors.border.subtle,
                )
            } else {
                null
            },
    ) {
        Column(
            modifier =
                if (padded) {
                    Modifier
                        .fillMaxSize()
                        .padding(HarvestCircleTheme.shell.layout.paneInset)
                } else {
                    Modifier.fillMaxSize()
                },
            content = content,
        )
    }
}
