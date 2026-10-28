package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HubViewState
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.harvestcircle.navigation.AppRoute

@Composable
fun HubWorkspace(
    route: AppRoute,
    state: HubViewState,
    initialize: () -> Unit,
    refresh: () -> Unit,
    openReferences: (() -> Unit)? = null,
) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(HarvestCircleTheme.shell.layout.contentGap)
            .testTag("hub-workspace"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.contentGap),
    ) {
        HarvestCircleShellPanel {
            HarvestCircleShellText("Food hub demonstration", role = HarvestCircleShellTextRole.SectionTitle)
            HarvestCircleShellText("Sample data only · One desktop · One operator")
            when (state) {
                HubViewState.Unavailable -> HarvestCircleShellText("Open an isolated demonstration using make demo DEMO_RUN=your-run.")
                HubViewState.SignedOut -> HarvestCircleShellText("Activate the hub owner's local Nostr identity to access this workspace.")
                HubViewState.Loading -> HarvestCircleShellText("Reading local hub…")
                HubViewState.NeedsSetup -> {
                    HarvestCircleShellText("Initialize this demo hub with the active identity as its local owner.")
                    HarvestCircleShellText(
                        "Use a newly generated demo identity and keep its recovery key. Ordinary credentials are never copied.",
                    )
                    HarvestCircleShellButton("Initialize demo hub", initialize, Modifier.testTag("hub-initialize"), primary = true)
                }
                is HubViewState.Failed -> {
                    HarvestCircleShellText(state.message)
                    state.submittedOperation?.let { HarvestCircleShellText("Original setup operation: ${it.value}") }
                    HarvestCircleShellButton("Check local hub", refresh, Modifier.testTag("hub-refresh"))
                }
                is HubViewState.Ready -> {
                    HarvestCircleShellText("Local hub ready", role = HarvestCircleShellTextRole.BodyStrong)
                    openReferences?.let { open ->
                        HarvestCircleShellButton("Reference data", open, Modifier.testTag("hub-references"), primary = true)
                    }

                    HarvestCircleShellText("Hub: ${state.context.id}")
                    HarvestCircleShellText("Owner: ${state.context.owner.value}")
                    HarvestCircleShellText(
                        when (route) {
                            AppRoute.HubWork -> "Handling work will appear here as stock workflows become available."
                            AppRoute.HubStock -> "Stock recording is not available in this build. No stock quantity is inferred."
                            else -> "Departure recording is not available in this build. No physical movement is inferred."
                        },
                    )
                }
            }
        }
    }
}
