package org.harvestcircle.application

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.identities.ui.HarvestCircleScreen
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.StartupFailureScreen
import org.harvestcircle.identities.ui.toUiModel
import java.util.concurrent.atomic.AtomicLong

internal typealias HarvestCirclePresenterFactory = (CoroutineScope) -> HarvestCirclePresenter

@Composable
fun HarvestCircleApplication(presenterFactory: HarvestCirclePresenterFactory = ::createHarvestCirclePresenter) {
    val scope = remember { CoroutineScope(SupervisorJob() + Dispatchers.Default) }
    val presenterResult = remember { runCatching { presenterFactory(scope) } }
    val presenter = presenterResult.getOrNull()
    if (presenter == null) {
        val message =
            (presenterResult.exceptionOrNull() as? ApplicationFailure)?.problem?.safeMessage
                ?: "The application could not start."
        StartupFailureScreen(message)
        return
    }
    val clipboard = remember { SecretClipboardController(scope) }
    val state by presenter.state.collectAsState()

    DisposableEffect(presenter, clipboard) {
        onDispose {
            clipboard.close()
            scope.launch {
                presenter.close()
                scope.cancel()
            }
        }
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

internal fun createHarvestCirclePresenter(scope: CoroutineScope): HarvestCirclePresenter {
    val developmentMode = java.lang.Boolean.getBoolean("harvestcircle.development")
    return HarvestCirclePresenter(
        runtime = NativeHarvestCircleRuntime.open(developmentMode),
        scope = scope,
        clock = DesktopApplicationClock,
        operationIds = DesktopOperationIdSource,
    )
}

private object DesktopApplicationClock : ApplicationClock {
    override fun now(): UnixSeconds = UnixSeconds(System.currentTimeMillis() / 1_000)
}

private object DesktopOperationIdSource : OperationIdSource {
    private val next = AtomicLong(1)

    override fun next(): OperationId = OperationId.from("desktop-operation:${next.getAndIncrement()}")
}
