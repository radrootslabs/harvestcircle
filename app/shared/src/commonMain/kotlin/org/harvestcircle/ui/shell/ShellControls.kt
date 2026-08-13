package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.VisualTransformation
import org.harvestcircle.designsystem.component.HarvestCircleButtonVariant
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.action.HarvestCircleButton
import org.harvestcircle.designsystem.component.container.HarvestCircleCard
import org.harvestcircle.designsystem.component.container.HarvestCircleCardPadding
import org.harvestcircle.designsystem.component.container.HarvestCircleCardVariant
import org.harvestcircle.designsystem.component.input.HarvestCircleTextField
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTab
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTabRow
import org.harvestcircle.designsystem.component.utility.HarvestCircleHorizontalDivider
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.component.HarvestCircleTextRole as DesignTextRole

enum class ShellTextRole { ScreenTitle, SectionTitle, CardTitle, Body, Secondary, Protocol, Button }

enum class ShellButtonKind { Primary, Secondary, Quiet, Destructive }

@Composable
fun ShellSurface(
    modifier: Modifier = Modifier,
    secondary: Boolean = false,
    content: @Composable () -> Unit,
) {
    HarvestCircleSurface(
        modifier = modifier,
        role = if (secondary) HarvestCircleSurfaceRole.Sunken else HarvestCircleSurfaceRole.Base,
    ) { content() }
}

@Composable
fun ShellText(
    text: String,
    modifier: Modifier = Modifier,
    textRole: ShellTextRole = ShellTextRole.Body,
) {
    val role =
        when (textRole) {
            ShellTextRole.ScreenTitle -> DesignTextRole.PageTitle
            ShellTextRole.SectionTitle -> DesignTextRole.SectionTitle
            ShellTextRole.CardTitle -> DesignTextRole.SubsectionTitle
            ShellTextRole.Body -> DesignTextRole.Body
            ShellTextRole.Secondary -> DesignTextRole.BodySmall
            ShellTextRole.Protocol -> DesignTextRole.Code
            ShellTextRole.Button -> DesignTextRole.Label
        }
    HarvestCircleText(
        text = text,
        modifier = modifier,
        role = role,
        tone = if (textRole == ShellTextRole.Secondary) HarvestCircleContentTone.Secondary else HarvestCircleContentTone.Inherit,
    )
}

@Composable
fun ShellButton(
    label: String,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    kind: ShellButtonKind = ShellButtonKind.Secondary,
) {
    HarvestCircleButton(
        onClick = onClick,
        modifier = modifier.semantics { contentDescription = description },
        enabled = enabled,
        variant = kind.toDesignVariant(),
    ) {
        HarvestCircleText(text = label, role = DesignTextRole.Label)
    }
}

@Composable
fun ShellTab(
    label: String,
    description: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    HarvestCircleTabRow {
        HarvestCircleTab(
            selected = selected,
            onClick = { if (!selected) onClick() },
            label = label,
            modifier = modifier.semantics { contentDescription = description },
            enabled = enabled,
        )
    }
}

@Composable
fun ShellIconButton(
    glyph: String,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    ShellButton(
        glyph,
        description,
        onClick,
        modifier,
        enabled,
        kind = ShellButtonKind.Quiet,
    )
}

@Composable
fun ShellTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    visualTransformation: VisualTransformation = VisualTransformation.None,
) {
    HarvestCircleTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = Modifier.fillMaxWidth(),
        inputModifier = modifier,
        label = label,
        placeholder = placeholder,
        enabled = enabled,
        visualTransformation = visualTransformation,
        accessibilityLabel = label,
    )
}

@Composable
fun ShellBadge(
    label: String,
    modifier: Modifier = Modifier,
) {
    HarvestCircleCard(
        modifier = modifier.semantics(mergeDescendants = true) {},
        variant = HarvestCircleCardVariant.Outlined,
        padding = HarvestCircleCardPadding.Compact,
    ) {
        HarvestCircleText(label, role = DesignTextRole.LabelSmall, tone = HarvestCircleContentTone.Secondary)
    }
}

@Composable
fun ShellCard(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    HarvestCircleCard(modifier = modifier) { content() }
}

@Composable
fun ShellDivider(modifier: Modifier = Modifier) {
    HarvestCircleHorizontalDivider(modifier)
}

private fun ShellButtonKind.toDesignVariant(): HarvestCircleButtonVariant =
    when (this) {
        ShellButtonKind.Primary -> HarvestCircleButtonVariant.Primary
        ShellButtonKind.Secondary -> HarvestCircleButtonVariant.Secondary
        ShellButtonKind.Quiet -> HarvestCircleButtonVariant.Ghost
        ShellButtonKind.Destructive -> HarvestCircleButtonVariant.Destructive
    }
