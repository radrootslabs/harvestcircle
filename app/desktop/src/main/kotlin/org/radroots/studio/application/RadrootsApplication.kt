package org.radroots.studio.application

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.CoroutineScope
import org.radroots.studio.accounts.ui.StudioScreen
import org.radroots.studio.accounts.ui.StudioUiActions
import org.radroots.studio.accounts.ui.StartupFailureScreen
import org.radroots.studio.accounts.ui.toUiModel
import org.radroots.studio.ffi.StudioAppCore
import org.radroots.studio.ffi.StudioException
import org.radroots.studio.ffi.compatibilityDescriptor

internal typealias StudioStoreFactory = (CoroutineScope) -> StudioAppStore

@Composable
fun RadrootsApplication(
    storeFactory: StudioStoreFactory = ::createStudioAppStore,
) {
    val scope = rememberCoroutineScope()
    val storeResult = remember { runCatching { storeFactory(scope) } }
    val store = storeResult.getOrNull()
    if (store == null) {
        val error = storeResult.exceptionOrNull()
        val message = (error as? StudioException.Failure)?.safeMessage
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

    StudioScreen(
        model = store.state.value.toUiModel(),
        actions = StudioUiActions(
            chooseCreateAccount = store::chooseCreateAccount,
            chooseImportAccount = store::chooseImportAccount,
            cancelAccountEntry = store::cancelAccountEntry,
            editImportDraft = store::editImportDraft,
            generateAccount = store::generateAccount,
            importSecretKey = store::importSecretKey,
            copyText = { value -> clipboard.copy(value) },
            acknowledgeGeneratedKeyBackup = store::acknowledgeGeneratedKeyBackup,
            selectAccount = store::selectAccount,
            activateAccount = store::activateAccount,
            requestAccountRemoval = store::requestAccountRemoval,
            cancelAccountRemoval = store::cancelAccountRemoval,
            confirmAccountRemoval = store::confirmAccountRemoval,
            refreshActiveProfile = store::refreshActiveProfile,
            signOut = store::signOut,
            showAccountChooser = store::showAccountChooser,
            hideAccountChooser = store::hideAccountChooser,
        ),
    )
}

internal fun createStudioAppStore(scope: CoroutineScope): StudioAppStore {
    val developmentMode = java.lang.Boolean.getBoolean("radroots.studio.development")
    val descriptor = compatibilityDescriptor()
    val core = StudioAppCore.openCompatible(
        expectation = verifyNativeCompatibility(descriptor),
        developmentMode = developmentMode,
    )
    return StudioAppStore(NativeStudioCoreGateway(core), scope)
}
