package org.radroots.studio.application

import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.CoroutineScope
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

    BasicText("radroots")
}

internal fun createStudioAppStore(scope: CoroutineScope): StudioAppStore {
    val developmentMode = java.lang.Boolean.getBoolean("radroots.studio.development")
    val core = StudioAppCore.open(developmentMode = developmentMode)
    return StudioAppStore(NativeStudioCoreGateway(core), scope)
}
