package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HarvestCirclePresenterState
import org.harvestcircle.application.HarvestCircleRoute
import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.harvestcircle.identities.ui.HarvestCircleUiActions

@Composable
fun ShellLifecycleCanvas(
    state: HarvestCirclePresenterState,
    actions: HarvestCircleUiActions,
) {
    val presentation = state.route.lifecyclePresentation()
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { ShellText(presentation.title, textRole = ShellTextRole.ScreenTitle) },
        body = {
            Column(
                Modifier.testTag("lifecycle-${state.route.name.lowercase()}"),
                verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
            ) {
                ShellText(presentation.detail)
                state.problem?.let { ShellText(it, Modifier.testTag("lifecycle-problem")) }
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
                if (state.lastProblem?.retryable == true) {
                    ShellButton("Retry", "Retry the last local operation", actions.retryLastCommand)
                }
                if (state.problem != null) {
                    ShellButton("Dismiss", "Dismiss this problem", actions.dismissProblem, kind = ShellButtonKind.Quiet)
                }
            }
        },
    )
}

@Composable
fun StartupFailureScreen(problem: String) {
    HarvestCircleTheme(AppearanceState()) {
        FailureCanvas(
            title = "HarvestCircle could not start",
            problem = problem,
            tag = "startup-failure",
        )
    }
}

@Composable
fun ShutdownFailureScreen(
    problem: String,
    forceExit: () -> Unit,
) {
    HarvestCircleTheme(AppearanceState()) {
        Box(Modifier.fillMaxSize().testTag("shutdown-failure")) {
            CanvasScaffold(
                textSize = TextSizePreference.Default,
                header = { ShellText("HarvestCircle could not close safely", textRole = ShellTextRole.ScreenTitle) },
                body = { ShellText(problem, Modifier.testTag("shutdown-problem")) },
                actionBar = {
                    ShellButton(
                        "Force exit",
                        "Force HarvestCircle to exit",
                        forceExit,
                        Modifier.testTag("force-exit"),
                        kind = ShellButtonKind.Destructive,
                    )
                },
            )
        }
    }
}

@Composable
private fun FailureCanvas(
    title: String,
    problem: String,
    tag: String,
) {
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { ShellText(title, textRole = ShellTextRole.ScreenTitle) },
        body = {
            Column(Modifier.testTag(tag)) {
                ShellText(problem, Modifier.testTag("startup-problem"))
            }
        },
        actionBar = {},
    )
}

private data class LifecyclePresentation(
    val title: String,
    val detail: String,
)

private fun HarvestCircleRoute.lifecyclePresentation(): LifecyclePresentation =
    when (this) {
        HarvestCircleRoute.OPENING -> LifecyclePresentation("Opening HarvestCircle", "Opening the local identity store.")
        HarvestCircleRoute.CHECKING_COMPATIBILITY ->
            LifecyclePresentation(
                "Checking compatibility",
                "Checking the native runtime contract.",
            )
        HarvestCircleRoute.ACQUIRING_OWNERSHIP -> LifecyclePresentation("Opening local data", "Acquiring exclusive local-store ownership.")
        HarvestCircleRoute.MIGRATING -> LifecyclePresentation("Updating local data", "Applying the supported local schema migration.")
        HarvestCircleRoute.RECOVERING -> LifecyclePresentation("Recovering local data", "Completing an interrupted local operation.")
        HarvestCircleRoute.DEGRADED, HarvestCircleRoute.BLOCKED ->
            LifecyclePresentation("Local data needs attention", "HarvestCircle cannot safely open the product shell.")
        HarvestCircleRoute.SHUTTING_DOWN -> LifecyclePresentation("Closing HarvestCircle", "Closing the native runtime safely.")
        HarvestCircleRoute.FATAL -> LifecyclePresentation("HarvestCircle could not start", "The local runtime reported a terminal problem.")
        HarvestCircleRoute.CLOSED -> LifecyclePresentation("HarvestCircle is closed", "The local runtime has closed.")
        HarvestCircleRoute.IDENTITIES, HarvestCircleRoute.ACTIVE_IDENTITY ->
            LifecyclePresentation("HarvestCircle", "Preparing the product shell.")
    }
