package org.radroots.studio.application

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import java.awt.Toolkit
import java.awt.datatransfer.StringSelection
import kotlinx.coroutines.CoroutineScope
import org.radroots.studio.accounts.ui.StudioScreen
import org.radroots.studio.accounts.ui.StudioUiActions
import org.radroots.studio.accounts.ui.toUiModel
import org.radroots.studio.ffi.StudioAppCore

internal typealias StudioStoreFactory = (CoroutineScope) -> StudioAppStore

@Composable
fun RadrootsApplication(
    storeFactory: StudioStoreFactory = ::createStudioAppStore,
) {
    val scope = rememberCoroutineScope()
    val store = remember { storeFactory(scope) }

    DisposableEffect(store) {
        onDispose(store::close)
    }

    StudioScreen(
        model = store.state.value.toUiModel(),
        actions = StudioUiActions(
            editImportDraft = store::editImportDraft,
            generateAccount = store::generateAccount,
            importSecretKey = store::importSecretKey,
            copyText = { value ->
                Toolkit.getDefaultToolkit().systemClipboard.setContents(StringSelection(value), null)
            },
            acknowledgeGeneratedKeyBackup = store::acknowledgeGeneratedKeyBackup,
            selectAccount = store::selectAccount,
            activateAccount = store::activateAccount,
            requestAccountRemoval = store::requestAccountRemoval,
            cancelAccountRemoval = store::cancelAccountRemoval,
            confirmAccountRemoval = store::confirmAccountRemoval,
        ),
    )
}

internal fun createStudioAppStore(scope: CoroutineScope): StudioAppStore {
    val developmentMode = java.lang.Boolean.getBoolean("radroots.studio.development")
    val core = StudioAppCore.open(developmentMode = developmentMode)
    return StudioAppStore(NativeStudioCoreGateway(core), scope)
}
