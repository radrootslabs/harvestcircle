package org.harvestcircle.ui.shell

import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.HarvestCircleIntent
import org.harvestcircle.application.HarvestCircleShellIntent
import org.harvestcircle.application.HarvestCircleShellState
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.application.ShellRoot
import org.harvestcircle.application.StatusOverlayKey
import org.harvestcircle.application.deriveShellStatus
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.toUiModel
import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.BootstrapStep
import org.harvestcircle.navigation.NavigationIntent
import org.harvestcircle.navigation.SettingsSection
import org.harvestcircle.product.ScreenKey
import org.harvestcircle.designsystem.theme.HarvestCircleTheme as HarvestCircleDesignTokens

@Composable
fun HarvestCircleShell(
    state: HarvestCircleShellState,
    identityActions: HarvestCircleUiActions,
    platformActions: HarvestCirclePlatformActions,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    HarvestCircleTheme(state.appearance) {
        val focusRegistry = remember { ShellFocusRegistry() }
        CompositionLocalProvider(LocalShellFocusRegistry provides focusRegistry) {
            ShellKeyboardHost(
                modal = state.overlays.current,
                onShortcut = { dispatchShortcut(it, state.overlays.current, dispatch) },
            ) {
                HarvestCircleShellContent(state, identityActions, platformActions, dispatch)
            }
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
    val modalOpen = state.overlays.current != null
    val fallback =
        if (state.root is ShellRoot.Dashboard) ShellFocusTarget.RouteFallback else ShellFocusTarget.BootstrapFallback
    Box(
        Modifier
            .fillMaxSize()
            .shellFocusTarget(ShellFocusTarget.BootstrapFallback)
            .focusable(state.root !is ShellRoot.Dashboard)
            .then(if (modalOpen) Modifier.clearAndSetSemantics {} else Modifier)
            .testTag("shell-background"),
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
    }
    ShellFocusRestorer(state.overlays.restoreFocus, fallback)
    FoundationOverlayHost(state.overlays, deriveShellStatus(state)) {
        dispatch(HarvestCircleShellIntent.Overlay(it))
    }
}

@Composable
private fun BootstrapWelcome(dispatch: (HarvestCircleShellIntent) -> Unit) {
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { ShellText("HarvestCircle", textRole = ShellTextRole.ScreenTitle) },
        body = {
            Column(
                Modifier.testTag("bootstrap-welcome"),
                verticalArrangement = Arrangement.spacedBy(HarvestCircleDesignTokens.shell.layout.contentGap),
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
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleDesignTokens.shell.layout.inlineGap)) {
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
                onIntent = { intent -> dispatchTopBar(intent, platformActions, dispatch) },
            )
        },
        sidebar = { WorkspaceSidebar(route.screenKey) { dispatch(HarvestCircleShellIntent.Navigate(it)) } },
        mainHeader = { MainPanelHeader(MainPanelHeaderModel(title = route.title())) },
        mainBody = {
            RouteFocusTarget(
                route.toString(),
                "${route.title()} main content",
            ) {
                when (route) {
                    AppRoute.PersonalToday ->
                        FoundationTodayScreen(
                            model = FoundationTodayModel(todayContext(state)),
                            openNostrReference = {
                                dispatch(
                                    HarvestCircleShellIntent.Overlay(
                                        OverlayIntent.OpenReference(ShellFocusTarget.TodayReference),
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
    overlay: FoundationOverlay?,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    val intent =
        when (shortcut) {
            ShellShortcut.Back -> HarvestCircleShellIntent.Navigation(NavigationIntent.Back)
            ShellShortcut.Forward -> HarvestCircleShellIntent.Navigation(NavigationIntent.Forward)
            ShellShortcut.OpenNostrReference ->
                HarvestCircleShellIntent.Overlay(OverlayIntent.OpenReference())
            ShellShortcut.Today -> HarvestCircleShellIntent.Navigate(ScreenKey.PersonalToday)
            ShellShortcut.Settings -> HarvestCircleShellIntent.Navigate(ScreenKey.Settings)
            ShellShortcut.CloseOverlay ->
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Escape((overlay as? FoundationOverlay.ConfirmAction)?.action),
                )
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
    platformActions: HarvestCirclePlatformActions,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    when (intent) {
        GlobalTopBarIntent.Back -> dispatch(HarvestCircleShellIntent.Navigation(NavigationIntent.Back))
        GlobalTopBarIntent.Forward -> dispatch(HarvestCircleShellIntent.Navigation(NavigationIntent.Forward))
        GlobalTopBarIntent.OpenNostrReference ->
            dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.OpenReference(ShellFocusTarget.TopBarReference)))
        GlobalTopBarIntent.ShowSyncStatus ->
            dispatch(
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Open(FoundationOverlay.Status(StatusOverlayKey.Sync), ShellFocusTarget.TopBarSync),
                ),
            )
        GlobalTopBarIntent.ShowSignerStatus ->
            dispatch(
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Open(FoundationOverlay.Status(StatusOverlayKey.Signer), ShellFocusTarget.TopBarSigner),
                ),
            )
        is GlobalTopBarIntent.SelectApplicationMenu ->
            when (intent.action) {
                ApplicationMenuAction.Settings -> openSettingsSection(SettingsSection.Appearance, dispatch)
                ApplicationMenuAction.AboutBuild -> openSettingsSection(SettingsSection.Project, dispatch)
                ApplicationMenuAction.Source -> platformActions.openSource()
                ApplicationMenuAction.Licence -> platformActions.openLicence()
            }
    }
}

private fun openSettingsSection(
    section: SettingsSection,
    dispatch: (HarvestCircleShellIntent) -> Unit,
) {
    dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.Settings))
    dispatch(
        HarvestCircleShellIntent.Navigation(
            NavigationIntent.SelectSettingsSection(section),
        ),
    )
}

private fun AppRoute.title(): String =
    when (this) {
        AppRoute.PersonalToday -> "Today"
        AppRoute.Network -> "Network"
        AppRoute.Settings -> "Settings"
        is AppRoute.Bootstrap -> "HarvestCircle"
    }
