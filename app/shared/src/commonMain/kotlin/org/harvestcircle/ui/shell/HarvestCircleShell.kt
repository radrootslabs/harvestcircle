package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.HarvestCircleIntent
import org.harvestcircle.application.HarvestCircleShellIntent
import org.harvestcircle.application.HarvestCircleShellState
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.ShellDestination
import org.harvestcircle.application.ShellRoot
import org.harvestcircle.application.SignerAvailability
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleScreen
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.toUiModel
import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.BootstrapStep
import org.harvestcircle.navigation.NavigationIntent

@Composable
fun HarvestCircleShell(
    state: HarvestCircleShellState,
    identityActions: HarvestCircleUiActions,
    platformActions: HarvestCirclePlatformActions,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    when (val root = state.root) {
        is ShellRoot.LifecycleCanvas ->
            HarvestCircleScreen(state.identity.toUiModel(), identityActions, platformActions)
        is ShellRoot.BootstrapCanvas ->
            when (root.step) {
                BootstrapStep.Welcome -> BootstrapWelcome(dispatch)
                BootstrapStep.CreateIdentity,
                BootstrapStep.ImportIdentity,
                ->
                    BootstrapIdentityEntry(
                        step = root.step,
                        model = state.identity.toUiModel(),
                        actions = identityActions,
                        onBack = {
                            identityActions.cancelIdentityEntry()
                            dispatch(
                                HarvestCircleShellIntent.Navigation(
                                    NavigationIntent.SelectBootstrapStep(BootstrapStep.Welcome),
                                ),
                            )
                        },
                    )
                BootstrapStep.GeneratedRecovery ->
                    GeneratedRecoveryCanvas(state.identity.toUiModel(), identityActions, platformActions)
                BootstrapStep.IdentityChooser,
                BootstrapStep.ActivationProgress,
                ->
                    IdentityChooserCanvas(
                        model = state.identity.toUiModel(),
                        actions = identityActions,
                        onReadOnly = { dispatch(HarvestCircleShellIntent.EnterReadOnly) },
                    )
            }
        is ShellRoot.Dashboard -> DashboardRoot(state, root, platformActions, dispatch)
    }
    FoundationOverlayHost(state.overlays) { dispatch(HarvestCircleShellIntent.Overlay(it)) }
}

@Composable
private fun BootstrapWelcome(dispatch: (HarvestCircleShellIntent) -> Unit) {
    CanvasScaffold(
        textSize = org.harvestcircle.design.TextSizePreference.Default,
        header = { BasicText("HarvestCircle") },
        body = {
            Column(
                Modifier.testTag("bootstrap-welcome"),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                BasicText("Coordinate local food with clear, signed terms.")
                BasicText("HarvestCircle helps farms and nearby buyers form one shared order.")
                BasicText("You do not need a HarvestCircle account.")
                BasicText("A farm opens a round")
                BasicText("The farm publishes the available boxes, pickup terms, and price levels.")
                BasicText("Buyers make private commitments")
                BasicText("Each buyer signs a maximum amount without publishing their identity.")
                BasicText("The authority clears the round")
                BasicText("The selected authority applies the farm’s signed terms and issues allocations.")
                BasicText("Open source · Nostr-based · No managed service required")
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                ShellAction("Explore read-only", "Explore read-only", "bootstrap-read-only") {
                    dispatch(HarvestCircleShellIntent.EnterReadOnly)
                }
                ShellAction("Create a local Nostr identity", "Create a local Nostr identity", "bootstrap-create") {
                    dispatch(HarvestCircleShellIntent.Identity(HarvestCircleIntent.ChooseCreateIdentity))
                    dispatch(
                        HarvestCircleShellIntent.Navigation(
                            NavigationIntent.SelectBootstrapStep(BootstrapStep.CreateIdentity),
                        ),
                    )
                }
                ShellAction("Import an existing identity", "Import an existing identity", "bootstrap-import") {
                    dispatch(HarvestCircleShellIntent.Identity(HarvestCircleIntent.ChooseImportIdentity))
                    dispatch(
                        HarvestCircleShellIntent.Navigation(
                            NavigationIntent.SelectBootstrapStep(BootstrapStep.ImportIdentity),
                        ),
                    )
                }
            }
        },
    )
}

@Composable
private fun DashboardRoot(
    state: HarvestCircleShellState,
    root: ShellRoot.Dashboard,
    platformActions: HarvestCirclePlatformActions,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    val route = root.navigation.current
    val destination =
        when (route) {
            AppRoute.PersonalToday -> ShellDestination.Today
            AppRoute.Network -> ShellDestination.Network
            is AppRoute.Settings -> ShellDestination.Settings
            is AppRoute.Bootstrap -> ShellDestination.Today
        }
    DashboardScaffold(
        windowWidthDp = ShellDimensions.PREFERRED_WINDOW_WIDTH_DP,
        inspectorVisible = false,
        topBar = {
            GlobalTopBar(
                model =
                    GlobalTopBarModel(
                        canGoBack = root.navigation.backStack.isNotEmpty(),
                        canGoForward = root.navigation.forwardStack.isNotEmpty(),
                        syncStatus = syncStatus(state),
                        signerStatus = signerStatus(state),
                    ),
                onIntent = { intent -> dispatchTopBar(intent, dispatch) },
            )
        },
        sidebar = { WorkspaceSidebar(destination) { dispatch(HarvestCircleShellIntent.Navigate(it)) } },
        mainHeader = { MainPanelHeader(MainPanelHeaderModel(title = route.title())) },
        mainBody = {
            when (route) {
                AppRoute.PersonalToday ->
                    FoundationTodayScreen(
                        model = FoundationTodayModel(todayContext(state)),
                        openNostrReference = {
                            dispatch(
                                HarvestCircleShellIntent.Overlay(
                                    OverlayIntent.Open(FoundationOverlay.OpenNostrReference()),
                                ),
                            )
                        },
                    )
                AppRoute.Network -> FoundationNetworkScreen(foundationNetworkModel(state))
                is AppRoute.Settings ->
                    FoundationSettingsScreen(
                        section = route.section,
                        appearance = state.appearance,
                        buildInfo = state.buildInfo,
                        actions =
                            FoundationSettingsActions(
                                selectSection = {
                                    dispatch(
                                        HarvestCircleShellIntent.Navigation(
                                            NavigationIntent.Navigate(AppRoute.Settings(it)),
                                        ),
                                    )
                                },
                                setTheme = { dispatch(HarvestCircleShellIntent.SetTheme(it)) },
                                setTextSize = { dispatch(HarvestCircleShellIntent.SetTextSize(it)) },
                                setMotion = { dispatch(HarvestCircleShellIntent.SetMotion(it)) },
                            ),
                        platformActions = platformActions,
                    )
                else -> BasicText(route.title(), Modifier.testTag("foundation-route-body"))
            }
        },
    )
}

private fun todayContext(state: HarvestCircleShellState): String =
    when {
        state.session.readOnly -> "Read-only session"
        state.identity.snapshot.activeIdentity != null ->
            state.identity.snapshot.activeIdentity.identity.displayLabel
        else -> "Signed out"
    }

private fun dispatchTopBar(
    intent: GlobalTopBarIntent,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    val shellIntent =
        when (intent) {
            GlobalTopBarIntent.Back -> HarvestCircleShellIntent.Navigation(NavigationIntent.Back)
            GlobalTopBarIntent.Forward -> HarvestCircleShellIntent.Navigation(NavigationIntent.Forward)
            GlobalTopBarIntent.OpenNostrReference ->
                HarvestCircleShellIntent.Overlay(OverlayIntent.Open(FoundationOverlay.OpenNostrReference()))
            GlobalTopBarIntent.ShowSyncStatus ->
                HarvestCircleShellIntent.Overlay(OverlayIntent.Open(FoundationOverlay.SyncStatus(SyncStatusLabel.NotYetObserved)))
            GlobalTopBarIntent.ShowSignerStatus ->
                HarvestCircleShellIntent.Overlay(OverlayIntent.Open(FoundationOverlay.SignerStatus(SignerStatusLabel.SignedOut)))
            GlobalTopBarIntent.OpenApplicationMenu ->
                HarvestCircleShellIntent.Navigate(ShellDestination.Settings)
        }
    dispatch(shellIntent)
}

private fun syncStatus(state: HarvestCircleShellState): SyncStatusLabel =
    when (
        state.identity.snapshot.activeIdentity
            ?.relays
            ?.state
    ) {
        org.harvestcircle.application.RelayConnectionState.Connected -> SyncStatusLabel.Available
        org.harvestcircle.application.RelayConnectionState.Degraded -> SyncStatusLabel.Degraded
        org.harvestcircle.application.RelayConnectionState.Error -> SyncStatusLabel.Unavailable
        else -> SyncStatusLabel.NotYetObserved
    }

private fun signerStatus(state: HarvestCircleShellState): SignerStatusLabel =
    when {
        state.session.readOnly -> SignerStatusLabel.ReadOnly
        state.identity.snapshot.activeIdentity == null -> SignerStatusLabel.SignedOut
        state.identity.snapshot.activeIdentity.identity.signer.availability == SignerAvailability.Available ->
            SignerStatusLabel.Available
        else -> SignerStatusLabel.CredentialMissing
    }

private fun AppRoute.title(): String =
    when (this) {
        AppRoute.PersonalToday -> "Today"
        AppRoute.Network -> "Network"
        is AppRoute.Settings -> "Settings"
        is AppRoute.Bootstrap -> "HarvestCircle"
    }
