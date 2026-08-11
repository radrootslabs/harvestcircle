package org.harvestcircle.application

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import org.harvestcircle.ffi.generateOperationIdV7
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleScreen
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.ShutdownFailureScreen
import org.harvestcircle.identities.ui.StartupFailureScreen
import org.harvestcircle.identities.ui.toUiModel
import java.util.concurrent.atomic.AtomicBoolean

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
    val presenterResult = remember { runCatching { presenterFactory(scope) } }
    val presenter = presenterResult.getOrNull()
    val clipboard = remember(presenter, clipboardFactory) { presenter?.let { clipboardFactory(scope) } }
    val lifecycle =
        remember(scope, presenter, clipboard, shutdownTimeoutMillis) {
            ApplicationLifecycleResources(
                applicationScope = scope,
                clipboard = clipboard,
                closePresenter = presenter?.let { active -> suspend { active.close() } },
                shutdownTimeoutMillis = shutdownTimeoutMillis,
            )
        }
    DisposableEffect(lifecycle) {
        onDispose { lifecycle.dispose() }
    }
    if (presenter == null) {
        LaunchedEffect(closeRequested) {
            if (closeRequested) onExitApproved()
        }
        val message =
            (presenterResult.exceptionOrNull() as? ApplicationFailure)?.problem?.safeMessage
                ?: "The application could not start."
        StartupFailureScreen(message)
        return
    }
    checkNotNull(clipboard)
    val state by presenter.state.collectAsState()
    var shutdownProblem by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(closeRequested, presenter) {
        if (closeRequested) {
            lifecycle.closeClipboard()
            val receipt = withTimeoutOrNull(shutdownTimeoutMillis) { presenter.close() }
            if (receipt?.closed == true) {
                lifecycle.completeNormalClose()
                onExitApproved()
            } else {
                shutdownProblem = "Native shutdown did not complete within the safe timeout."
            }
        }
    }

    shutdownProblem?.let { problem ->
        ShutdownFailureScreen(problem = problem, forceExit = onExitApproved)
        return
    }

    HarvestCircleScreen(
        model = state.toUiModel(),
        actions =
            HarvestCircleUiActions(
                chooseCreateIdentity = { presenter.dispatch(HarvestCircleIntent.ChooseCreateIdentity) },
                chooseImportIdentity = { presenter.dispatch(HarvestCircleIntent.ChooseImportIdentity) },
                cancelIdentityEntry = { presenter.dispatch(HarvestCircleIntent.CancelIdentityEntry) },
                editImportDraft = { presenter.dispatch(HarvestCircleIntent.EditImportDraft(it)) },
                generateIdentity = { presenter.dispatch(HarvestCircleIntent.GenerateIdentity) },
                importSecretKey = { presenter.dispatch(HarvestCircleIntent.ImportIdentity) },
                acknowledgeGeneratedKeyBackup = { presenter.dispatch(HarvestCircleIntent.AcknowledgeGeneratedRecovery) },
                cancelGeneratedKeyBackup = { presenter.dispatch(HarvestCircleIntent.CancelGeneratedRecovery) },
                selectIdentity = { presenter.dispatch(HarvestCircleIntent.SelectIdentity(IdentityId.fromPublicKeyHex(it))) },
                activateIdentity = { presenter.dispatch(HarvestCircleIntent.ActivateIdentity(IdentityId.fromPublicKeyHex(it))) },
                requestIdentityRemoval = {
                    presenter.dispatch(HarvestCircleIntent.RequestIdentityRemoval(IdentityId.fromPublicKeyHex(it)))
                },
                cancelIdentityRemoval = { presenter.dispatch(HarvestCircleIntent.CancelIdentityRemoval) },
                confirmIdentityRemoval = { presenter.dispatch(HarvestCircleIntent.ConfirmIdentityRemoval) },
                refreshActiveProfile = { presenter.dispatch(HarvestCircleIntent.RefreshActiveProfile) },
                retryLastCommand = { presenter.dispatch(HarvestCircleIntent.RetryLastCommand) },
                signOut = { presenter.dispatch(HarvestCircleIntent.SignOut) },
                showIdentityChooser = { presenter.dispatch(HarvestCircleIntent.ShowIdentityChooser) },
                hideIdentityChooser = { presenter.dispatch(HarvestCircleIntent.HideIdentityChooser) },
            ),
        platformActions = HarvestCirclePlatformActions(copySecret = clipboard::copy),
    )
}

internal const val DEFAULT_SHUTDOWN_TIMEOUT_MILLIS = 5_000L

internal class ApplicationLifecycleResources(
    private val applicationScope: CoroutineScope,
    private val clipboard: AutoCloseable?,
    private val closePresenter: (suspend () -> ShutdownReceipt?)?,
    private val shutdownTimeoutMillis: Long,
    private val fallbackDispatcher: CoroutineDispatcher = Dispatchers.Default,
) {
    private val clipboardClosed = AtomicBoolean(false)
    private val normalCloseCompleted = AtomicBoolean(false)
    private val disposed = AtomicBoolean(false)

    init {
        require(shutdownTimeoutMillis > 0L) { "Shutdown timeout must be positive" }
    }

    fun closeClipboard() {
        if (clipboardClosed.compareAndSet(false, true)) clipboard?.close()
    }

    fun completeNormalClose() {
        normalCloseCompleted.set(true)
        closeClipboard()
        applicationScope.cancel()
    }

    fun dispose() {
        if (!disposed.compareAndSet(false, true)) return
        closeClipboard()
        applicationScope.cancel()
        val close = closePresenter ?: return
        if (normalCloseCompleted.get()) return

        val fallbackJob = SupervisorJob()
        CoroutineScope(fallbackJob + fallbackDispatcher).launch {
            try {
                withTimeoutOrNull(shutdownTimeoutMillis) { close() }
            } finally {
                fallbackJob.cancel()
            }
        }
    }
}

internal fun createHarvestCirclePresenter(scope: CoroutineScope): HarvestCirclePresenter {
    val developmentMode = java.lang.Boolean.getBoolean("harvestcircle.development")
    return HarvestCirclePresenter(
        runtime = NativeHarvestCircleRuntime.open(desktopRuntimeOpenConfiguration(developmentMode)),
        scope = scope,
        clock = DesktopApplicationClock,
        operationIds = DesktopOperationIdSource,
    )
}

private object DesktopApplicationClock : ApplicationClock {
    override fun now(): UnixSeconds = UnixSeconds(System.currentTimeMillis() / 1_000)
}

private object DesktopOperationIdSource : OperationIdSource {
    override fun next(): OperationId = OperationId.from(generateOperationIdV7())
}
