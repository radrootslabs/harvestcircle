package org.harvestcircle.designsystem.shell

import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

public enum class HarvestCircleShellTextRole {
    PageTitle,
    SectionTitle,
    PaneTitle,
    Body,
    BodyStrong,
    Small,
    Label,
    Code,
}

@Composable
public fun HarvestCircleShellText(
    text: String,
    modifier: Modifier = Modifier,
    role: HarvestCircleShellTextRole = HarvestCircleShellTextRole.Body,
    color: Color = HarvestCircleShellPalette.contentPrimary,
    maxLines: Int = Int.MAX_VALUE,
    overflow: TextOverflow = TextOverflow.Clip,
) {
    val typography = HarvestCircleTheme.foundation.typography
    val base =
        when (role) {
            HarvestCircleShellTextRole.PageTitle -> typography.pageTitle
            HarvestCircleShellTextRole.SectionTitle -> typography.sectionTitle
            HarvestCircleShellTextRole.PaneTitle -> typography.bodyStrong.copy(fontWeight = FontWeight.SemiBold)
            HarvestCircleShellTextRole.Body -> typography.body
            HarvestCircleShellTextRole.BodyStrong -> typography.bodyStrong
            HarvestCircleShellTextRole.Small -> typography.bodySmall
            HarvestCircleShellTextRole.Label -> typography.label
            HarvestCircleShellTextRole.Code -> typography.code
        }
    BasicText(text, modifier, base.copy(color = color), maxLines = maxLines, overflow = overflow)
}
