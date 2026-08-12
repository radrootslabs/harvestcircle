package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.HarvestCircleIntent
import org.harvestcircle.application.HarvestCircleShellIntent
import org.harvestcircle.application.HarvestCircleShellState
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.ShellRoot
import org.harvestcircle.application.StatusOverlayKey
import org.harvestcircle.application.deriveShellStatus
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.toUiModel
import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.BootstrapStep
import org.harvestcircle.navigation.NavigationIntent
import org.harvestcircle.product.ScreenKey

@Composable
fun HarvestCircleShell(
    state: HarvestCircleShellState,
    identityActions: HarvestCircleUiActions,
    platformActions: HarvestCirclePlatformActions,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    HarvestCircleTheme(state.appearance) {
        ShellKeyboardHost(onShortcut = { dispatchShortcut(it, dispatch) }) {
            HarvestCircleShellContent(state, identityActions, platformActions, dispatch)
        }
    }
}

@Composable
private fun HarvestCircleShellContent(
    state: HarvestCircleShellState,
    identityActions: HarvestCircleUiActions,
    platformActions: HarvestCirclePlatformActions,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    when (val root = state.root) {
        is ShellRoot.LifecycleCanvas ->
            ShellLifecycleCanvas(state.identity, identityActions)
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
        is ShellRoot.Dashboard -> DashboardRoot(state, root, identityActions, platformActions, dispatch)
    }
    FoundationOverlayHost(state.overlays, deriveShellStatus(state), state.identity.busy) {
        dispatch(HarvestCircleShellIntent.Overlay(it))
    }
}

@Composable
private fun BootstrapWelcome(dispatch: (HarvestCircleShellIntent) -> Unit) {
    CanvasScaffold(
        textSize = org.harvestcircle.design.TextSizePreference.Default,
        header = { ShellText("HarvestCircle", textRole = ShellTextRole.ScreenTitle) },
        body = {
            Column(
                Modifier.testTag("bootstrap-welcome"),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                ShellText("Coordinate local food with clear, signed terms.", textRole = ShellTextRole.SectionTitle)
                ShellText("HarvestCircle helps farms and nearby buyers form one shared order.")
                ShellText("You do not need a HarvestCircle account.")
                ShellText("A farm opens a round", textRole = ShellTextRole.CardTitle)
                ShellText("The farm publishes the available boxes, pickup terms, and price levels.")
                ShellText("Buyers make private commitments", textRole = ShellTextRole.CardTitle)
                ShellText("Each buyer signs a maximum amount without publishing their identity.")
                ShellText("The authority clears the round", textRole = ShellTextRole.CardTitle)
                ShellText("The selected authority applies the farm’s signed terms and issues allocations.")
                ShellText("Open source · Nostr-based · No managed service required", textRole = ShellTextRole.Secondary)
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
    identityActions: HarvestCircleUiActions,
    platformActions: HarvestCirclePlatformActions,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    val route = root.navigation.current
    val status = deriveShellStatus(state)
    DashboardScaffold(
        inspectorVisible = false,
        topBar = {
            GlobalTopBar(
                model =
                    GlobalTopBarModel(
                        canGoBack = root.navigation.backStack.isNotEmpty(),
                        canGoForward = root.navigation.forwardStack.isNotEmpty(),
                        syncStatus = status.sync,
                        signerStatus = status.signer,
                    ),
                onIntent = { intent -> dispatchTopBar(intent, dispatch) },
            )
        },
        sidebar = { WorkspaceSidebar(route.screenKey) { dispatch(HarvestCircleShellIntent.Navigate(it)) } },
        mainHeader = { MainPanelHeader(MainPanelHeaderModel(title = route.title())) },
        mainBody = {
            RouteFocusTarget(
                route.toString(),
                "${route.title()} main content",
                restoreFocus = state.overlays.current == null,
            ) {
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
                    AppRoute.Network ->
                        FoundationNetworkScreen(
                            foundationNetworkModel(state),
                            refreshProfile = identityActions.refreshActiveProfile,
                            signOut = identityActions.signOut,
                        )
                    AppRoute.Settings ->
                        FoundationSettingsScreen(
                            section = root.navigation.settings.section,
                            appearance = state.appearance,
                            buildInfo = state.buildInfo,
                            actions =
                                FoundationSettingsActions(
                                    selectSection = {
                                        dispatch(
                                            HarvestCircleShellIntent.Navigation(
                                                NavigationIntent.SelectSettingsSection(it),
                                            ),
                                        )
                                    },
                                    setTheme = { dispatch(HarvestCircleShellIntent.SetTheme(it)) },
                                    setTextSize = { dispatch(HarvestCircleShellIntent.SetTextSize(it)) },
                                    setMotion = { dispatch(HarvestCircleShellIntent.SetMotion(it)) },
                                ),
                            platformActions = platformActions,
                        )
                    else -> ShellText(route.title(), Modifier.testTag("foundation-route-body"))
                }
            }
        },
    )
}

private fun dispatchShortcut(
    shortcut: ShellShortcut,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    val intent =
        when (shortcut) {
            ShellShortcut.Back -> HarvestCircleShellIntent.Navigation(NavigationIntent.Back)
            ShellShortcut.Forward -> HarvestCircleShellIntent.Navigation(NavigationIntent.Forward)
            ShellShortcut.OpenNostrReference ->
                HarvestCircleShellIntent.Overlay(OverlayIntent.Open(FoundationOverlay.OpenNostrReference()))
            ShellShortcut.Today -> HarvestCircleShellIntent.Navigate(ScreenKey.PersonalToday)
            ShellShortcut.Settings -> HarvestCircleShellIntent.Navigate(ScreenKey.Settings)
            ShellShortcut.CloseOverlay -> HarvestCircleShellIntent.Overlay(OverlayIntent.Escape())
        }
    dispatch(intent)
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
                HarvestCircleShellIntent.Overlay(OverlayIntent.Open(FoundationOverlay.Status(StatusOverlayKey.Sync)))
            GlobalTopBarIntent.ShowSignerStatus ->
                HarvestCircleShellIntent.Overlay(OverlayIntent.Open(FoundationOverlay.Status(StatusOverlayKey.Signer)))
            GlobalTopBarIntent.OpenApplicationMenu ->
                HarvestCircleShellIntent.Navigate(ScreenKey.Settings)
        }
    dispatch(shellIntent)
}

private fun AppRoute.title(): String =
    when (this) {
        AppRoute.PersonalToday -> "Today"
        AppRoute.Network -> "Network"
        AppRoute.Settings -> "Settings"
        is AppRoute.Bootstrap -> "HarvestCircle"
    }
