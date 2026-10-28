package org.harvestcircle.ui.shell

import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.application.DispatchPresenter
import org.harvestcircle.application.DispatchViewState
import org.harvestcircle.application.FoundationOverlay
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HandlingViewState
import org.harvestcircle.application.HarvestCircleIntent
import org.harvestcircle.application.HarvestCircleShellIntent
import org.harvestcircle.application.HarvestCircleShellState
import org.harvestcircle.application.HubViewState
import org.harvestcircle.application.OverlayIntent
import org.harvestcircle.application.ReferencePresenter
import org.harvestcircle.application.ReferenceViewState
import org.harvestcircle.application.ShellFocusTarget
import org.harvestcircle.application.ShellRoot
import org.harvestcircle.application.StatusOverlayKey
import org.harvestcircle.application.StockPresenter
import org.harvestcircle.application.StockViewState
import org.harvestcircle.application.deriveShellStatus
import org.harvestcircle.designsystem.layout.HarvestCircleFrameLayoutClass
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPalette
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
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
    hubState: HubViewState = HubViewState.Unavailable,
    initializeHub: () -> Unit = {},
    refreshHub: () -> Unit = {},
    stockState: StockViewState = StockViewState(),
    stockPresenter: StockPresenter? = null,
    submitReceipt: () -> Unit = {},
    referenceState: ReferenceViewState = ReferenceViewState(),
    referencePresenter: ReferencePresenter? = null,
    submitReference: () -> Unit = {},
    handlingState: HandlingViewState = HandlingViewState(),
    handlingPresenter: HandlingPresenter? = null,
    submitHandling: () -> Unit = {},
    dispatchState: DispatchViewState = DispatchViewState(),
    dispatchPresenter: DispatchPresenter? = null,
    submitDispatch: () -> Unit = {},
) {
    HarvestCircleTheme(state.appearance) {
        val focusRegistry = remember { ShellFocusRegistry() }
        CompositionLocalProvider(LocalShellFocusRegistry provides focusRegistry) {
            ShellKeyboardHost(
                modal = state.overlays.current,
                onShortcut = { dispatchShortcut(it, state.overlays.current, dispatch) },
            ) {
                HarvestCircleShellContent(
                    state,
                    identityActions,
                    platformActions,
                    dispatch,
                    hubState,
                    initializeHub,
                    refreshHub,
                    stockState,
                    stockPresenter,
                    submitReceipt,
                    referenceState,
                    referencePresenter,
                    submitReference,
                    handlingState,
                    handlingPresenter,
                    submitHandling,
                    dispatchState,
                    dispatchPresenter,
                    submitDispatch,
                )
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
    hubState: HubViewState = HubViewState.Unavailable,
    initializeHub: () -> Unit = {},
    refreshHub: () -> Unit = {},
    stockState: StockViewState = StockViewState(),
    stockPresenter: StockPresenter? = null,
    submitReceipt: () -> Unit = {},
    referenceState: ReferenceViewState = ReferenceViewState(),
    referencePresenter: ReferencePresenter? = null,
    submitReference: () -> Unit = {},
    handlingState: HandlingViewState = HandlingViewState(),
    handlingPresenter: HandlingPresenter? = null,
    submitHandling: () -> Unit = {},
    dispatchState: DispatchViewState = DispatchViewState(),
    dispatchPresenter: DispatchPresenter? = null,
    submitDispatch: () -> Unit = {},
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
                    BootstrapStep.IdentityChooser ->
                        IdentityChooserCanvas(
                            model = state.identity.toUiModel(),
                            actions = identityActions,
                            onReadOnly = { dispatch(HarvestCircleShellIntent.EnterReadOnly) },
                        )
                    BootstrapStep.ActivationProgress ->
                        IdentityActivationCanvas(
                            model = state.identity.toUiModel(),
                            activatingPublicKeyHex = checkNotNull(state.identity.activatingIdentityId).value,
                        )
                }
            is ShellRoot.Dashboard ->
                DashboardRoot(
                    state,
                    root,
                    identityActions,
                    platformActions,
                    dispatch,
                    hubState,
                    initializeHub,
                    refreshHub,
                    stockState,
                    stockPresenter,
                    submitReceipt,
                    referenceState,
                    referencePresenter,
                    submitReference,
                    handlingState,
                    handlingPresenter,
                    submitHandling,
                    dispatchState,
                    dispatchPresenter,
                    submitDispatch,
                )
        }
    }
    ShellFocusRestorer(state.overlays.restoreFocus, fallback)
    FoundationOverlayHost(
        state = state.overlays,
        status = deriveShellStatus(state),
        showBanner = state.root !is ShellRoot.Dashboard,
        onAddOrActivateIdentity = { dispatch(HarvestCircleShellIntent.ManageIdentity) },
    ) {
        dispatch(HarvestCircleShellIntent.Overlay(it))
    }
}

@Composable
private fun BootstrapWelcome(dispatch: (HarvestCircleShellIntent) -> Unit) {
    CanvasScaffold(
        textSize = TextSizePreference.Default,
        header = { HarvestCircleShellText("HarvestCircle", role = HarvestCircleShellTextRole.PaneTitle) },
        body = {
            Column(
                Modifier.testTag("bootstrap-welcome"),
                verticalArrangement = Arrangement.spacedBy(HarvestCircleDesignTokens.shell.layout.contentGap),
            ) {
                HarvestCircleShellPanel {
                    HarvestCircleShellText(
                        "See what arrived, where it is, and where it went.",
                        role = HarvestCircleShellTextRole.SectionTitle,
                    )
                    HarvestCircleShellText("HarvestCircle is preparing a local food-hub demonstration using fictional stock.")
                    HarvestCircleShellText("You do not need a HarvestCircle account.")
                }
                HarvestCircleShellPanel {
                    HarvestCircleShellText("Receive original whole packs", role = HarvestCircleShellTextRole.BodyStrong)
                    HarvestCircleShellText("Keep source references and physical locations visible.")
                    HarvestCircleShellText("Handle stock deliberately", role = HarvestCircleShellTextRole.BodyStrong)
                    HarvestCircleShellText("Track acceptance, restrictions and handling work separately.")
                    HarvestCircleShellText("Record actual departures", role = HarvestCircleShellTextRole.BodyStrong)
                    HarvestCircleShellText("Preserve recipients and history without treating drafts as physical events.")
                }
                HarvestCircleShellText(
                    "Open source · Nostr-based · No managed service required",
                    role = HarvestCircleShellTextRole.Small,
                    color = HarvestCircleShellPalette.contentSecondary,
                )
            }
        },
        actionBar = {
            Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleDesignTokens.shell.layout.inlineGap)) {
                HarvestCircleShellButton(
                    "Explore read-only",
                    { dispatch(HarvestCircleShellIntent.EnterReadOnly) },
                    Modifier.testTag("bootstrap-read-only"),
                    primary = true,
                )
                HarvestCircleShellButton(
                    "Create a local Nostr identity",
                    {
                        dispatch(HarvestCircleShellIntent.Identity(HarvestCircleIntent.ChooseCreateIdentity))
                        dispatch(
                            HarvestCircleShellIntent.Navigation(
                                NavigationIntent.SelectBootstrapStep(BootstrapStep.CreateIdentity),
                            ),
                        )
                    },
                    Modifier.testTag("bootstrap-create"),
                )
                HarvestCircleShellButton(
                    "Import an existing identity",
                    {
                        dispatch(HarvestCircleShellIntent.Identity(HarvestCircleIntent.ChooseImportIdentity))
                        dispatch(
                            HarvestCircleShellIntent.Navigation(
                                NavigationIntent.SelectBootstrapStep(BootstrapStep.ImportIdentity),
                            ),
                        )
                    },
                    Modifier.testTag("bootstrap-import"),
                )
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
    hubState: HubViewState = HubViewState.Unavailable,
    initializeHub: () -> Unit = {},
    refreshHub: () -> Unit = {},
    stockState: StockViewState = StockViewState(),
    stockPresenter: StockPresenter? = null,
    submitReceipt: () -> Unit = {},
    referenceState: ReferenceViewState = ReferenceViewState(),
    referencePresenter: ReferencePresenter? = null,
    submitReference: () -> Unit = {},
    handlingState: HandlingViewState = HandlingViewState(),
    handlingPresenter: HandlingPresenter? = null,
    submitHandling: () -> Unit = {},
    dispatchState: DispatchViewState = DispatchViewState(),
    dispatchPresenter: DispatchPresenter? = null,
    submitDispatch: () -> Unit = {},
) {
    val route = root.navigation.current
    val status = deriveShellStatus(state)
    var networkSection by remember { mutableStateOf(NetworkSection.Overview) }
    DashboardScaffold(
        inspectorVisible = false,
        sidebarCollapsed = false,
        topBar = { geometry ->
            GlobalTopBar(
                model =
                    GlobalTopBarModel(
                        canGoBack = root.navigation.backStack.isNotEmpty(),
                        canGoForward = root.navigation.forwardStack.isNotEmpty(),
                        syncStatus = status.sync,
                        signerStatus = status.signer,
                        selectedScreen = if (route == AppRoute.HubReferences) ScreenKey.HubWork else route.screenKey,
                    ),
                onIntent = { intent -> dispatchTopBar(intent, platformActions, dispatch) },
                compact = geometry.layoutClass != HarvestCircleFrameLayoutClass.Expanded,
                showSidebarToggle = false,
                sidebarCollapsed = geometry.sidebarWidth == HarvestCircleDesignTokens.shell.frame.collapsedSidebarWidth,
                onToggleSidebar = {},
            )
        },
        sidebar = { geometry ->
            WorkspaceSidebar(
                selected = if (route == AppRoute.HubReferences) ScreenKey.HubWork else route.screenKey,
                onScreen = { dispatch(HarvestCircleShellIntent.Navigate(it)) },
                compact = geometry.sidebarWidth == HarvestCircleDesignTokens.shell.frame.collapsedSidebarWidth,
                onToggleCollapsed = {},
                showCollapseControl = false,
                sessionLabel = todayContext(state),
                chromeClearance = geometry.sidebarChromeClearance,
                topBandFullyExcluded = geometry.sidebarTopBandFullyExcluded,
            )
        },
        mainHeader = {
            when (route) {
                AppRoute.Network ->
                    MainPanelHeader(
                        MainPanelHeaderModel(
                            title = route.title(),
                            tabs = NetworkSection.entries.map { TemplateTab(TemplateSelectionKey(it.key), it.label) },
                            selectedTab = TemplateSelectionKey(networkSection.key),
                        ),
                        onTabSelected = { selected ->
                            networkSection = NetworkSection.entries.single { it.key == selected.value }
                        },
                    )
                AppRoute.Settings ->
                    MainPanelHeader(
                        MainPanelHeaderModel(
                            title = route.title(),
                            tabs =
                                SettingsSection.entries.map {
                                    TemplateTab(TemplateSelectionKey(it.name.lowercase()), it.name)
                                },
                            selectedTab =
                                TemplateSelectionKey(
                                    root.navigation.settings.section.name
                                        .lowercase(),
                                ),
                        ),
                        onTabSelected = { selected ->
                            dispatch(
                                HarvestCircleShellIntent.Navigation(
                                    NavigationIntent.SelectSettingsSection(
                                        SettingsSection.entries.single { it.name.equals(selected.value, ignoreCase = true) },
                                    ),
                                ),
                            )
                        },
                    )
                else -> MainPanelHeader(MainPanelHeaderModel(title = route.title()))
            }
        },
        mainBody = {
            Column(Modifier.fillMaxSize()) {
                FoundationStatusBanner(status, Modifier.fillMaxWidth())
                Box(Modifier.weight(1f).fillMaxSize()) {
                    RouteFocusTarget(
                        route.toString(),
                        "${route.title()} main content",
                    ) {
                        when (route) {
                            AppRoute.HubWork, AppRoute.HubStock -> {
                                if (hubState is HubViewState.Ready && stockPresenter != null && stockState.available) {
                                    if (handlingPresenter != null && handlingState.available) {
                                        OperationalWorkspace(
                                            route,
                                            stockState,
                                            stockPresenter,
                                            submitReceipt,
                                            handlingState,
                                            handlingPresenter,
                                            submitHandling,
                                            openStock = { dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.HubStock)) },
                                            openReferences = { dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.HubReferences)) },
                                        )
                                    } else {
                                        StockWorkspace(
                                            route,
                                            stockState,
                                            stockPresenter,
                                            submitReceipt,
                                            openReferences = { dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.HubReferences)) },
                                        )
                                    }
                                } else {
                                    HubWorkspace(route, hubState, initializeHub, refreshHub)
                                }
                            }
                            AppRoute.HubDispatch -> {
                                if (hubState is HubViewState.Ready && dispatchPresenter != null && dispatchState.available) {
                                    LaunchedEffect(route) {
                                        dispatchPresenter.park()
                                        dispatchPresenter.refresh()
                                    }
                                    DispatchWorkspace(
                                        dispatchState,
                                        dispatchPresenter,
                                        submitDispatch,
                                        references = { dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.HubReferences)) },
                                    )
                                } else {
                                    HubWorkspace(route, hubState, initializeHub, refreshHub)
                                }
                            }
                            AppRoute.HubReferences -> {
                                if (hubState is HubViewState.Ready && referencePresenter != null) {
                                    ReferenceWorkspace(
                                        referenceState,
                                        referencePresenter,
                                        submitReference,
                                        returnToWork = { dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.HubWork)) },
                                    )
                                } else {
                                    HubWorkspace(route, hubState, initializeHub, refreshHub)
                                }
                            }
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
                                    section = networkSection,
                                    onSectionSelected = { networkSection = it },
                                    showSectionTabs = false,
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
                                    showSectionTabs = false,
                                )
                            else -> HarvestCircleShellText(route.title(), Modifier.testTag("foundation-route-body"))
                        }
                    }
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
            ShellShortcut.Work -> HarvestCircleShellIntent.Navigate(ScreenKey.HubWork)
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
        GlobalTopBarIntent.OpenWork -> dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.HubWork))
        GlobalTopBarIntent.OpenNetwork -> dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.Network))
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
        AppRoute.HubReferences -> "Reference data"
        AppRoute.HubWork -> "Work"
        AppRoute.HubStock -> "Stock"
        AppRoute.HubDispatch -> "Dispatch"
        AppRoute.PersonalToday -> "Today"
        AppRoute.Network -> "Network"
        AppRoute.Settings -> "Settings"
        is AppRoute.Bootstrap -> "HarvestCircle"
    }
