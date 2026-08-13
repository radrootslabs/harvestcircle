package org.harvestcircle.designsystem.component.input

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleControlSize
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.component.HarvestCircleIconSize
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.action.HarvestCircleIconButton
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import org.harvestcircle.designsystem.internal.chrome.HarvestCircleMacFocusFrame
import org.harvestcircle.designsystem.internal.chrome.harvestCircleMacControlBrush
import org.harvestcircle.designsystem.internal.interaction.harvestCircleHoverable
import org.harvestcircle.designsystem.internal.interaction.harvestCircleInteractions
import org.harvestcircle.designsystem.internal.interaction.rememberHarvestCircleInteractionSources
import org.harvestcircle.designsystem.primitive.HarvestCircleIcon
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.primitive.ProvideHarvestCircleContentColor
import org.harvestcircle.designsystem.theme.HarvestCircleInputMode
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.jetbrains.compose.resources.DrawableResource

private val LocalHarvestCircleTextFieldShape = compositionLocalOf<Shape?> { null }
private val LocalHarvestCircleTextFieldFocusRingShape = compositionLocalOf<Shape?> { null }

/**
 * Canonical AppKit-style text field.
 *
 * Labels sit above the bezel and placeholders remain inside it. Text editing, IME composition,
 * selection, cursor, keyboard actions, and read-only behavior come from Foundation's
 * [BasicTextField] rather than Material text-field anatomy. The optional visible focus ring is
 * disabled by default.
 */
@Composable
public fun HarvestCircleTextField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    inputModifier: Modifier = Modifier,
    label: String? = null,
    placeholder: String? = null,
    supportingText: String? = null,
    errorMessage: String? = null,
    leadingIcon: DrawableResource? = null,
    trailingIcon: (@Composable () -> Unit)? = null,
    enabled: Boolean = true,
    readOnly: Boolean = false,
    singleLine: Boolean = true,
    minLines: Int = 1,
    maxLines: Int = if (singleLine) 1 else Int.MAX_VALUE,
    visualTransformation: VisualTransformation = VisualTransformation.None,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    accessibilityLabel: String? = null,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
) {
    require(minLines >= 1) { "minLines must be at least 1" }
    require(maxLines >= minLines) { "maxLines must be greater than or equal to minLines" }

    val sources = rememberHarvestCircleInteractionSources()
    val interactions = sources.harvestCircleInteractions(enabled)
    val colors = HarvestCircleTheme.foundation.colors
    val fieldShape = LocalHarvestCircleTextFieldShape.current ?: HarvestCircleTheme.foundation.shapes.control
    val fieldFocusRingShape =
        LocalHarvestCircleTextFieldFocusRingShape.current ?: HarvestCircleTheme.foundation.shapes.controlFocusRing
    val resolvedAccessibilityLabel = accessibilityLabel ?: label ?: placeholder
    val isError = errorMessage != null
    val visualHeight =
        if (singleLine) {
            HarvestCircleTheme.shell.dimensions.controlMedium
        } else {
            HarvestCircleTheme.shell.dimensions.controlLarge
        }
    val background =
        when {
            !enabled -> colors.surface.sunken
            readOnly -> colors.surface.base
            else -> colors.surface.raised
        }
    val borderColor =
        when {
            isError -> colors.feedback.error.strong
            interactions.hovered && enabled -> colors.border.strong
            !enabled -> colors.border.subtle
            else -> colors.border.default
        }
    val textColor = if (enabled) colors.content.primary else colors.content.disabled
    val supportingColor =
        when {
            isError -> colors.feedback.error.strong
            enabled -> colors.content.muted
            else -> colors.content.disabled
        }

    Column(
        modifier = modifier,
        horizontalAlignment = Alignment.Start,
    ) {
        if (label != null) {
            ProvideHarvestCircleContentColor(
                when {
                    isError -> colors.feedback.error.strong
                    enabled -> colors.content.secondary
                    else -> colors.content.disabled
                },
            ) {
                HarvestCircleText(
                    text = label,
                    modifier = Modifier.padding(bottom = HarvestCircleTheme.foundation.spacing.xs),
                    role = HarvestCircleTextRole.LabelSmall,
                    tone = HarvestCircleContentTone.Inherit,
                )
            }
        }

        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            modifier =
                inputModifier
                    .fillMaxWidth()
                    .heightIn(min = HarvestCircleTheme.shell.dimensions.minimumInteractive)
                    .harvestCircleHoverable(sources = sources, enabled = enabled)
                    .semantics {
                        if (resolvedAccessibilityLabel != null) {
                            contentDescription = resolvedAccessibilityLabel
                        }
                        if (errorMessage != null) {
                            stateDescription = errorMessage
                        }
                    },
            enabled = enabled,
            readOnly = readOnly,
            textStyle =
                HarvestCircleTheme.foundation.typography.body
                    .merge(TextStyle(color = textColor)),
            keyboardOptions = keyboardOptions,
            keyboardActions = keyboardActions,
            singleLine = singleLine,
            maxLines = maxLines,
            minLines = minLines,
            visualTransformation = visualTransformation,
            interactionSource = sources.activationSource,
            cursorBrush = SolidColor(if (isError) colors.feedback.error.strong else colors.focus.ring),
            decorationBox = { innerTextField ->
                Box(
                    modifier =
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = HarvestCircleTheme.shell.dimensions.minimumInteractive),
                    contentAlignment = Alignment.Center,
                ) {
                    HarvestCircleMacFocusFrame(
                        focused = interactions.focused && enabled,
                        focusRing = focusRing,
                        modifier = Modifier.fillMaxWidth(),
                        shape = fieldShape,
                        ringShape = fieldFocusRingShape,
                    ) {
                        Box(
                            modifier =
                                Modifier
                                    .fillMaxWidth()
                                    .heightIn(min = visualHeight)
                                    .clip(fieldShape)
                                    .background(
                                        brush = harvestCircleMacControlBrush(background),
                                        shape = fieldShape,
                                    ).border(
                                        width = 1.dp,
                                        color = borderColor,
                                        shape = fieldShape,
                                    ),
                        ) {
                            ProvideHarvestCircleContentColor(textColor) {
                                Row(
                                    modifier =
                                        Modifier
                                            .fillMaxWidth()
                                            .heightIn(min = visualHeight)
                                            .padding(horizontal = HarvestCircleTheme.foundation.spacing.md),
                                    verticalAlignment =
                                        if (singleLine) Alignment.CenterVertically else Alignment.Top,
                                ) {
                                    if (leadingIcon != null) {
                                        HarvestCircleIcon(
                                            resource = leadingIcon,
                                            contentDescription = null,
                                            size = HarvestCircleIconSize.Medium,
                                            tone =
                                                if (enabled) {
                                                    HarvestCircleContentTone.Secondary
                                                } else {
                                                    HarvestCircleContentTone.Disabled
                                                },
                                        )
                                        Spacer(Modifier.width(HarvestCircleTheme.foundation.spacing.sm))
                                    }

                                    Box(
                                        modifier =
                                            Modifier
                                                .weight(1f)
                                                .padding(
                                                    vertical =
                                                        if (singleLine) {
                                                            0.dp
                                                        } else {
                                                            HarvestCircleTheme.foundation.spacing.sm
                                                        },
                                                ),
                                        contentAlignment =
                                            if (singleLine) Alignment.CenterStart else Alignment.TopStart,
                                    ) {
                                        if (value.isEmpty() && placeholder != null) {
                                            HarvestCircleText(
                                                text = placeholder,
                                                role = HarvestCircleTextRole.Body,
                                                tone =
                                                    if (enabled) {
                                                        HarvestCircleContentTone.Muted
                                                    } else {
                                                        HarvestCircleContentTone.Disabled
                                                    },
                                                maxLines = if (singleLine) 1 else maxLines,
                                            )
                                        }
                                        innerTextField()
                                    }

                                    if (trailingIcon != null) {
                                        Spacer(Modifier.width(HarvestCircleTheme.foundation.spacing.sm))
                                        ProvideHarvestCircleContentColor(
                                            if (enabled) {
                                                colors.content.secondary
                                            } else {
                                                colors.content.disabled
                                            },
                                        ) {
                                            trailingIcon()
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
        )

        val supporting = errorMessage ?: supportingText
        if (supporting != null) {
            ProvideHarvestCircleContentColor(supportingColor) {
                HarvestCircleText(
                    text = supporting,
                    modifier = Modifier.padding(top = HarvestCircleTheme.foundation.spacing.xs),
                    role = HarvestCircleTextRole.BodySmall,
                    tone = HarvestCircleContentTone.Inherit,
                    maxLines = 3,
                )
            }
        }
    }
}

/** Search-specific field with canonical magnifier and clear controls. */
@Composable
public fun HarvestCircleSearchField(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    clearLabel: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
) {
    CompositionLocalProvider(
        LocalHarvestCircleTextFieldShape provides HarvestCircleTheme.foundation.shapes.pill,
        LocalHarvestCircleTextFieldFocusRingShape provides HarvestCircleTheme.foundation.shapes.pillFocusRing,
    ) {
        HarvestCircleTextField(
            value = value,
            onValueChange = onValueChange,
            modifier = modifier,
            placeholder = placeholder,
            leadingIcon = HarvestCircleIcons.Search,
            trailingIcon =
                if (value.isNotEmpty()) {
                    {
                        HarvestCircleIconButton(
                            onClick = { onValueChange("") },
                            icon = HarvestCircleIcons.Close,
                            label = clearLabel,
                            modifier =
                                Modifier.size(
                                    if (HarvestCircleTheme.component.inputMode == HarvestCircleInputMode.Touch) {
                                        HarvestCircleTheme.shell.dimensions.minimumInteractive
                                    } else {
                                        HarvestCircleTheme.shell.dimensions.controlSmall
                                    },
                                ),
                            size = HarvestCircleControlSize.Small,
                            enabled = enabled,
                            focusRing = focusRing,
                        )
                    }
                } else {
                    null
                },
            enabled = enabled,
            singleLine = true,
            accessibilityLabel = placeholder,
            focusRing = focusRing,
        )
    }
}
