package org.harvestcircle.designsystem.component.feedback

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import org.harvestcircle.designsystem.internal.progress.HarvestCircleMacIndeterminateBar
import org.harvestcircle.designsystem.internal.progress.HarvestCircleMacSpinner
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

public enum class HarvestCircleProgressKind {
    Circular,
    Linear,
}

/** Canonical indeterminate macOS spinner or progress bar, rendered without Material indicators. */
@Composable
public fun HarvestCircleProgressIndicator(
    modifier: Modifier = Modifier,
    kind: HarvestCircleProgressKind = HarvestCircleProgressKind.Circular,
) {
    when (kind) {
        HarvestCircleProgressKind.Circular ->
            HarvestCircleMacSpinner(
                modifier =
                    modifier.semantics {
                        progressBarRangeInfo = ProgressBarRangeInfo.Indeterminate
                    },
                size = HarvestCircleTheme.shell.dimensions.iconLarge,
                color = HarvestCircleTheme.foundation.colors.content.secondary,
            )

        HarvestCircleProgressKind.Linear ->
            HarvestCircleMacIndeterminateBar(
                modifier =
                    modifier
                        .fillMaxWidth()
                        .semantics {
                            progressBarRangeInfo = ProgressBarRangeInfo.Indeterminate
                        },
                color = HarvestCircleTheme.foundation.colors.action.primary.rest,
            )
    }
}
