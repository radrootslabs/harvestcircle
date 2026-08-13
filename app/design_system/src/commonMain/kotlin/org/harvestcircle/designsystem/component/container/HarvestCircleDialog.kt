package org.harvestcircle.designsystem.component.container

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import org.harvestcircle.designsystem.component.HarvestCircleButtonVariant
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.action.HarvestCircleButton
import org.harvestcircle.designsystem.focus.HarvestCircleFocusDismissBehavior
import org.harvestcircle.designsystem.internal.focus.harvestCircleClearFocusOnBackgroundPress
import org.harvestcircle.designsystem.primitive.HarvestCircleSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleSurfaceRole
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Generic dialog frame for product-owned forms and interaction flows. */
@Composable
public fun HarvestCircleDialogFrame(
    onDismissRequest: () -> Unit,
    title: String,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Dialog(onDismissRequest = onDismissRequest) {
        HarvestCircleSurface(
            modifier =
                modifier
                    .widthIn(min = 360.dp, max = 480.dp)
                    .semantics { paneTitle = title },
            role = HarvestCircleSurfaceRole.Overlay,
            shape = HarvestCircleTheme.foundation.shapes.dialog,
            border = BorderStroke(1.dp, HarvestCircleTheme.foundation.colors.border.subtle),
            shadowElevation = HarvestCircleTheme.component.elevations.dialog,
        ) {
            Column(
                modifier = Modifier.padding(HarvestCircleTheme.foundation.spacing.xl),
                verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.lg),
                content = content,
            )
        }
    }
}

/** Canonical macOS alert sheet/panel treatment implemented with Compose Dialog primitives. */
@Composable
public fun HarvestCircleDialog(
    onDismissRequest: () -> Unit,
    title: String,
    message: String,
    confirmLabel: String,
    onConfirm: () -> Unit,
    modifier: Modifier = Modifier,
    dismissLabel: String? = null,
    onDismiss: (() -> Unit)? = null,
    destructive: Boolean = false,
    focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.WhenFocused,
    focusDismissBehavior: HarvestCircleFocusDismissBehavior =
        HarvestCircleFocusDismissBehavior.ClearOnBackgroundPress,
    forceFocusDismissal: Boolean = false,
) {
    require((dismissLabel == null) == (onDismiss == null)) {
        "dismissLabel and onDismiss must either both be supplied or both be null"
    }

    val defaultFocusRequester = remember { FocusRequester() }
    val focusDismiss = destructive && dismissLabel != null

    val focusManager = LocalFocusManager.current
    LaunchedEffect(defaultFocusRequester, focusDismiss) {
        defaultFocusRequester.requestFocus()
    }

    HarvestCircleDialogFrame(
        onDismissRequest = onDismissRequest,
        title = title,
        modifier =
            modifier.harvestCircleClearFocusOnBackgroundPress(
                focusManager = focusManager,
                enabled =
                    focusDismissBehavior ==
                        HarvestCircleFocusDismissBehavior.ClearOnBackgroundPress,
                force = forceFocusDismissal,
            ),
    ) {
        Column(
            verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.sm),
        ) {
            HarvestCircleText(
                text = title,
                role = HarvestCircleTextRole.SectionTitle,
            )
            HarvestCircleText(
                text = message,
                role = HarvestCircleTextRole.Body,
                tone = HarvestCircleContentTone.Secondary,
            )
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md, Alignment.End),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (dismissLabel != null && onDismiss != null) {
                HarvestCircleButton(
                    onClick = onDismiss,
                    modifier =
                        if (focusDismiss) {
                            Modifier.focusRequester(defaultFocusRequester)
                        } else {
                            Modifier
                        },
                    variant = HarvestCircleButtonVariant.Secondary,
                    focusRing = focusRing,
                ) {
                    HarvestCircleText(
                        text = dismissLabel,
                        role = HarvestCircleTextRole.Label,
                        tone = HarvestCircleContentTone.Inherit,
                    )
                }
            }

            HarvestCircleButton(
                onClick = onConfirm,
                modifier =
                    if (!focusDismiss) {
                        Modifier.focusRequester(defaultFocusRequester)
                    } else {
                        Modifier
                    },
                variant =
                    if (destructive) {
                        HarvestCircleButtonVariant.Destructive
                    } else {
                        HarvestCircleButtonVariant.Primary
                    },
                focusRing = focusRing,
            ) {
                HarvestCircleText(
                    text = confirmLabel,
                    role = HarvestCircleTextRole.Label,
                    tone = HarvestCircleContentTone.Inherit,
                )
            }
        }
    }
}
