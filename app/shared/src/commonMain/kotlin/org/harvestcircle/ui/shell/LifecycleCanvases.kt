package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.application.HarvestCirclePresenterState
import org.harvestcircle.application.HarvestCircleRoute
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.identities.ui.HarvestCircleUiActions

@Composable
fun ShellLifecycleCanvas(
    state: HarvestCirclePresenterState,
    actions: HarvestCircleUiActions,
) {
    val presentation = state.route.lifecyclePresentation()
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { HarvestCircleShellText(presentation.title, role = HarvestCircleShellTextRole.PaneTitle) },
        body = {
            HarvestCircleShellPanel(Modifier.testTag("lifecycle-${state.route.name.lowercase()}")) {
                HarvestCircleShellText(presentation.detail, role = HarvestCircleShellTextRole.SectionTitle)
                state.problem?.let { HarvestCircleShellText(it, Modifier.testTag("lifecycle-problem")) }
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                if (state.lastProblem?.retryable == true) {
                    HarvestCircleShellButton("Retry", actions.retryLastCommand, primary = true)
                }
                if (state.problem != null) HarvestCircleShellButton("Dismiss", actions.dismissProblem)
            }
        },
    )
}

@Composable
fun StartupProgressScreen(closing: Boolean = false) {
    HarvestCircleTheme(AppearanceState()) {
        FailureCanvas(
            if (closing) "Closing HarvestCircle" else "Opening HarvestCircle",
            if (closing) "Waiting for the local runtime to close safely." else "Preparing local data.",
            "startup-progress",
        )
    }
}

@Composable
fun StartupFailureScreen(
    problem: String,
    close: (() -> Unit)? = null,
    retry: (() -> Unit)? = null,
    copyDiagnostics: (() -> Unit)? = null,
    copyStatus: String? = null,
) {
    HarvestCircleTheme(AppearanceState()) {
        FailureCanvas("HarvestCircle could not start", problem, "startup-failure") {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                retry?.let { HarvestCircleShellButton("Try again", it, primary = true) }
                copyDiagnostics?.let { HarvestCircleShellButton("Copy diagnostics", it) }
                close?.let { HarvestCircleShellButton("Close", it) }
            }
            copyStatus?.let { HarvestCircleShellText(it) }
        }
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
                header = { HarvestCircleShellText("HarvestCircle could not close safely", role = HarvestCircleShellTextRole.PaneTitle) },
                body = {
                    HarvestCircleShellPanel { HarvestCircleShellText(problem, Modifier.testTag("shutdown-problem")) }
                },
                actionBar = {
                    HarvestCircleShellButton(
                        "Force exit",
                        forceExit,
                        Modifier.testTag("force-exit"),
                        destructive = true,
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
    actions: @Composable () -> Unit = {},
) {
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { HarvestCircleShellText(title, role = HarvestCircleShellTextRole.PaneTitle) },
        body = {
            HarvestCircleShellPanel(Modifier.testTag(tag)) {
                HarvestCircleShellText(problem, Modifier.testTag("startup-problem"))
            }
        },
        actionBar = actions,
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
