package org.radroots.harvestcircle.application

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.CoroutineScope
import org.radroots.harvestcircle.accounts.ui.HarvestCircleScreen
import org.radroots.harvestcircle.accounts.ui.HarvestCircleUiActions
import org.radroots.harvestcircle.accounts.ui.StartupFailureScreen
import org.radroots.harvestcircle.accounts.ui.toUiModel
import org.radroots.harvestcircle.ffi.HarvestCircleAppCore
import org.radroots.harvestcircle.ffi.HarvestCircleException
import org.radroots.harvestcircle.ffi.compatibilityDescriptor

internal typealias HarvestCircleStoreFactory = (CoroutineScope) -> HarvestCircleAppStore

@Composable
fun HarvestCircleApplication(storeFactory: HarvestCircleStoreFactory = ::createHarvestCircleAppStore) {
    val scope = rememberCoroutineScope()
    val storeResult = remember { runCatching { storeFactory(scope) } }
    val store = storeResult.getOrNull()
    if (store == null) {
        val error = storeResult.exceptionOrNull()
        val message =
            (error as? HarvestCircleException.Failure)?.safeMessage
                ?: "The application could not start."
        StartupFailureScreen(message)
        return
    }
    val clipboard = remember { SecretClipboardController(scope) }

    DisposableEffect(store, clipboard) {
        onDispose {
            clipboard.close()
            store.close()
        }
    }

    HarvestCircleScreen(
        model = store.state.value.toUiModel(),
        actions =
            HarvestCircleUiActions(
                chooseCreateAccount = store::chooseCreateAccount,
                chooseImportAccount = store::chooseImportAccount,
                cancelAccountEntry = store::cancelAccountEntry,
                editImportDraft = store::editImportDraft,
                generateAccount = store::generateAccount,
                importSecretKey = store::importSecretKey,
                copyText = { value -> clipboard.copy(value) },
                acknowledgeGeneratedKeyBackup = store::acknowledgeGeneratedKeyBackup,
                cancelGeneratedKeyBackup = store::cancelGeneratedKeyBackup,
                selectAccount = store::selectAccount,
                activateAccount = store::activateAccount,
                requestAccountRemoval = store::requestAccountRemoval,
                cancelAccountRemoval = store::cancelAccountRemoval,
                confirmAccountRemoval = store::confirmAccountRemoval,
                refreshActiveProfile = store::refreshActiveProfile,
                retryLastCommand = store::retryLastCommand,
                signOut = store::signOut,
                showAccountChooser = store::showAccountChooser,
                hideAccountChooser = store::hideAccountChooser,
            ),
    )
}

internal fun createHarvestCircleAppStore(scope: CoroutineScope): HarvestCircleAppStore {
    val developmentMode = java.lang.Boolean.getBoolean("harvestcircle.development")
    val descriptor = compatibilityDescriptor()
    val core =
        HarvestCircleAppCore.openCompatible(
            expectation = verifyNativeCompatibility(descriptor),
            developmentMode = developmentMode,
        )
    return HarvestCircleAppStore(NativeHarvestCircleCoreGateway(core), scope)
}
