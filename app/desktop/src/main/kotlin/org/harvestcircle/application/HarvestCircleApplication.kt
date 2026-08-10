package org.harvestcircle.application

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.CoroutineScope
import org.harvestcircle.ffi.HarvestCircleAppCore
import org.harvestcircle.ffi.HarvestCircleException
import org.harvestcircle.ffi.compatibilityDescriptor
import org.harvestcircle.identities.ui.HarvestCircleScreen
import org.harvestcircle.identities.ui.HarvestCircleUiActions
import org.harvestcircle.identities.ui.StartupFailureScreen
import org.harvestcircle.identities.ui.toUiModel

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
                chooseCreateIdentity = store::chooseCreateIdentity,
                chooseImportIdentity = store::chooseImportIdentity,
                cancelIdentityEntry = store::cancelIdentityEntry,
                editImportDraft = store::editImportDraft,
                generateIdentity = store::generateIdentity,
                importSecretKey = store::importSecretKey,
                copyText = { value -> clipboard.copy(value) },
                acknowledgeGeneratedKeyBackup = store::acknowledgeGeneratedKeyBackup,
                cancelGeneratedKeyBackup = store::cancelGeneratedKeyBackup,
                selectIdentity = store::selectIdentity,
                activateIdentity = store::activateIdentity,
                requestIdentityRemoval = store::requestIdentityRemoval,
                cancelIdentityRemoval = store::cancelIdentityRemoval,
                confirmIdentityRemoval = store::confirmIdentityRemoval,
                refreshActiveProfile = store::refreshActiveProfile,
                retryLastCommand = store::retryLastCommand,
                signOut = store::signOut,
                showIdentityChooser = store::showIdentityChooser,
                hideIdentityChooser = store::hideIdentityChooser,
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
