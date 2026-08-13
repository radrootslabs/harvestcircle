package org.harvestcircle.designsystem.primitive

import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

@Composable
private fun textColor(tone: HarvestCircleContentTone): Color =
    when (tone) {
        HarvestCircleContentTone.Primary -> HarvestCircleTheme.foundation.colors.content.primary
        HarvestCircleContentTone.Secondary -> HarvestCircleTheme.foundation.colors.content.secondary
        HarvestCircleContentTone.Muted -> HarvestCircleTheme.foundation.colors.content.muted
        HarvestCircleContentTone.Disabled -> HarvestCircleTheme.foundation.colors.content.disabled
        HarvestCircleContentTone.Inverse -> HarvestCircleTheme.foundation.colors.content.inverse
        HarvestCircleContentTone.Inherit -> currentHarvestCircleContentColor()
    }

@Composable
private fun textStyle(role: HarvestCircleTextRole): TextStyle =
    when (role) {
        HarvestCircleTextRole.Display -> HarvestCircleTheme.foundation.typography.display
        HarvestCircleTextRole.PageTitle -> HarvestCircleTheme.foundation.typography.pageTitle
        HarvestCircleTextRole.SectionTitle -> HarvestCircleTheme.foundation.typography.sectionTitle
        HarvestCircleTextRole.SubsectionTitle -> HarvestCircleTheme.foundation.typography.subsectionTitle
        HarvestCircleTextRole.Body -> HarvestCircleTheme.foundation.typography.body
        HarvestCircleTextRole.BodyStrong -> HarvestCircleTheme.foundation.typography.bodyStrong
        HarvestCircleTextRole.BodySmall -> HarvestCircleTheme.foundation.typography.bodySmall
        HarvestCircleTextRole.Label -> HarvestCircleTheme.foundation.typography.label
        HarvestCircleTextRole.LabelSmall -> HarvestCircleTheme.foundation.typography.labelSmall
        HarvestCircleTextRole.Code -> HarvestCircleTheme.foundation.typography.code
    }

/** Canonical HarvestCircle text primitive rendered without Material component anatomy. */
@Composable
public fun HarvestCircleText(
    text: String,
    modifier: Modifier = Modifier,
    role: HarvestCircleTextRole = HarvestCircleTextRole.Body,
    tone: HarvestCircleContentTone = HarvestCircleContentTone.Inherit,
    textAlign: TextAlign? = null,
    overflow: TextOverflow = TextOverflow.Clip,
    softWrap: Boolean = true,
    maxLines: Int = Int.MAX_VALUE,
    minLines: Int = 1,
) {
    val baseStyle = textStyle(role)
    val style =
        if (textAlign == null) {
            baseStyle.copy(color = textColor(tone))
        } else {
            baseStyle.copy(
                color = textColor(tone),
                textAlign = textAlign,
            )
        }

    BasicText(
        text = text,
        modifier = modifier,
        style = style,
        overflow = overflow,
        softWrap = softWrap,
        maxLines = maxLines,
        minLines = minLines,
    )
}
