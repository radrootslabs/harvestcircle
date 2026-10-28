package org.harvestcircle.application

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.harvestcircle.ffi.generateOperationIdV7
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.ui.shell.HarvestCircleShell
import org.harvestcircle.ui.shell.ShutdownFailureScreen
import org.harvestcircle.ui.shell.StartupFailureScreen
import org.harvestcircle.ui.shell.StartupProgressScreen

internal typealias HarvestCirclePresenterFactory = (CoroutineScope) -> HarvestCirclePresenter
internal typealias SecretClipboardFactory = (CoroutineScope) -> SecretClipboardController

@Composable
fun HarvestCircleApplication(
    closeRequested: Boolean = false,
    onExitApproved: () -> Unit = {},
    shutdownTimeoutMillis: Long = DEFAULT_SHUTDOWN_TIMEOUT_MILLIS,
    presenterFactory: HarvestCirclePresenterFactory = ::createHarvestCirclePresenter,
) = HarvestCircleApplicationWithDependencies(
    closeRequested = closeRequested,
    onExitApproved = onExitApproved,
    shutdownTimeoutMillis = shutdownTimeoutMillis,
    clipboardFactory = ::SecretClipboardController,
    presenterFactory = presenterFactory,
)

@Composable
internal fun HarvestCircleApplicationWithDependencies(
    closeRequested: Boolean = false,
    onExitApproved: () -> Unit = {},
    shutdownTimeoutMillis: Long = DEFAULT_SHUTDOWN_TIMEOUT_MILLIS,
    clipboardFactory: SecretClipboardFactory,
    presenterFactory: HarvestCirclePresenterFactory,
) {
    val scope = remember { CoroutineScope(SupervisorJob() + Dispatchers.Default) }
    val startup =
        remember {
            StartupController(scope, presenterFactory, { it.close()?.closed == true }, shutdownTimeoutMillis)
        }
    val clipboard = remember { clipboardFactory(scope) }
    DisposableEffect(startup) {
        startup.start()
        onDispose {
            clipboard.close()
            startup.requestClose()
        }
    }
    LaunchedEffect(closeRequested) {
        if (closeRequested) {
            clipboard.close()
            startup.requestClose()
        }
    }
    val startupState by startup.state.collectAsState()
    LaunchedEffect(startupState) {
        if (startupState == StartupState.Closed) onExitApproved()
    }
    val presenter =
        when (val state = startupState) {
            is StartupState.Ready -> state.value
            is StartupState.Failed -> {
                var copyStatus by remember(state) { mutableStateOf<String?>(null) }
                StartupFailureScreen(
                    problem = state.problem.safeMessage,
                    close = startup::requestClose,
                    retry = if (state.canRetry) startup::start else null,
                    copyDiagnostics = {
                        copyStatus =
                            when (clipboard.copy(state.diagnostics)) {
                                SecretClipboardResult.Copied -> "Diagnostics copied."
                                SecretClipboardResult.Unavailable -> "Clipboard unavailable. Diagnostics are in the application log."
                            }
                    },
                    copyStatus = copyStatus,
                )
                return
            }
            StartupState.CloseFailed -> {
                ShutdownFailureScreen("Native shutdown did not complete within the safe timeout.", onExitApproved)
                return
            }
            else -> {
                StartupProgressScreen(closing = startupState != StartupState.Opening)
                return
            }
        }
    val shellPresenter =
        remember(presenter, scope) {
            HarvestCircleShellPresenter(presenter, presenter.buildInfo, scope, NativeNostrReferenceParser)
        }
    DisposableEffect(shellPresenter) { onDispose { shellPresenter.close() } }
    val shellState by shellPresenter.state.collectAsState()
    val hubScope = rememberCoroutineScope()
    val hubIdentity =
        shellState.identity.snapshot.activeIdentity
            ?.identity
            ?.id
    val hubPresenter = remember(presenter, hubIdentity) { HubPresenter(presenter.hubRuntime, hubScope, DesktopOperationIdSource) }
    LaunchedEffect(hubPresenter, hubIdentity) { hubPresenter.bindIdentity(hubIdentity) }
    DisposableEffect(hubPresenter) { onDispose { hubPresenter.close() } }
    val hubState by hubPresenter.state.collectAsState()
    val referencePresenter =
        remember(presenter, hubIdentity) { ReferencePresenter(presenter.referenceRuntime, hubScope, DesktopOperationIdSource) }
    DisposableEffect(referencePresenter) { onDispose { referencePresenter.close() } }
    LaunchedEffect(referencePresenter, hubState) {
        (hubState as? HubViewState.Ready)?.context?.let { referencePresenter.attach(it.id) }
    }
    val referenceState by referencePresenter.state.collectAsState()

    val identityActions =
        HarvestCircleUiActions(
            chooseCreateIdentity = { presenter.dispatch(HarvestCircleIntent.ChooseCreateIdentity) },
            chooseImportIdentity = { presenter.dispatch(HarvestCircleIntent.ChooseImportIdentity) },
            cancelIdentityEntry = { presenter.dispatch(HarvestCircleIntent.CancelIdentityEntry) },
            editImportDraft = { presenter.dispatch(HarvestCircleIntent.EditImportDraft.from(it)) },
            generateIdentity = { presenter.dispatch(HarvestCircleIntent.GenerateIdentity) },
            importSecretKey = { presenter.dispatch(HarvestCircleIntent.ImportIdentity) },
            acknowledgeGeneratedKeyBackup = { presenter.dispatch(HarvestCircleIntent.AcknowledgeGeneratedRecovery) },
            cancelGeneratedKeyBackup = { presenter.dispatch(HarvestCircleIntent.CancelGeneratedRecovery) },
            selectIdentity = { presenter.dispatch(HarvestCircleIntent.SelectIdentity(IdentityId.fromPublicKeyHex(it))) },
            activateIdentity = { presenter.dispatch(HarvestCircleIntent.ActivateIdentity(IdentityId.fromPublicKeyHex(it))) },
            requestIdentityRemoval = {
                presenter.dispatch(HarvestCircleIntent.RequestIdentityRemoval(IdentityId.fromPublicKeyHex(it)))
            },
            refreshActiveProfile = { presenter.dispatch(HarvestCircleIntent.RefreshActiveProfile) },
            retryLastCommand = { presenter.dispatch(HarvestCircleIntent.RetryLastCommand) },
            dismissProblem = { presenter.dispatch(HarvestCircleIntent.DismissProblem) },
            signOut = { presenter.dispatch(HarvestCircleIntent.SignOut) },
            showIdentityChooser = { presenter.dispatch(HarvestCircleIntent.ShowIdentityChooser) },
            hideIdentityChooser = { presenter.dispatch(HarvestCircleIntent.HideIdentityChooser) },
        )
    HarvestCircleShell(
        state = shellState,
        identityActions = identityActions,
        platformActions =
            HarvestCirclePlatformActions(
                copySecret = clipboard::copy,
                openSource = { openProjectLink(ProjectLink.Source) },
                openLicence = { openProjectLink(ProjectLink.Licence) },
            ),
        dispatch = shellPresenter::dispatch,
        hubState = hubState,
        initializeHub = { hubPresenter.initialize(shellState.identity.snapshot.revision) },
        refreshHub = hubPresenter::refresh,
        referenceState = referenceState,
        referencePresenter = referencePresenter,
        submitReference = { referencePresenter.submit(shellState.identity.snapshot.revision) },
    )
}

internal const val DEFAULT_SHUTDOWN_TIMEOUT_MILLIS = 5_000L

internal fun createHarvestCirclePresenter(scope: CoroutineScope): HarvestCirclePresenter = createPresenter(scope, null)

internal fun createDemoHarvestCirclePresenter(
    scope: CoroutineScope,
    runName: String,
): HarvestCirclePresenter = createPresenter(scope, runName)

private fun createPresenter(
    scope: CoroutineScope,
    demoRun: String?,
): HarvestCirclePresenter {
    val developmentMode = java.lang.Boolean.getBoolean("harvestcircle.development")
    val runtime =
        if (demoRun == null) {
            NativeHarvestCircleRuntime.open(desktopRuntimeOpenConfiguration(developmentMode))
        } else {
            NativeHarvestCircleRuntime.openDemo(demoRun)
        }
    try {
        return HarvestCirclePresenter(
            runtime = runtime,
            scope = scope,
            clock = DesktopApplicationClock,
            operationIds = DesktopOperationIdSource,
        )
    } catch (error: Exception) {
        throw StartupConstructionFailure(error) { runtime.shutdown().closed }
    } catch (error: LinkageError) {
        throw StartupConstructionFailure(error) { runtime.shutdown().closed }
    }
}

private object DesktopApplicationClock : ApplicationClock {
    override fun now(): UnixSeconds = UnixSeconds(System.currentTimeMillis() / 1_000)
}

private object DesktopOperationIdSource : OperationIdSource {
    override fun next(): OperationId = OperationId.from(generateOperationIdV7())
}
