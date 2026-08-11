package org.harvestcircle.application

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.harvestcircle.design.AppearanceState
import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.TextSizePreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.navigation.NavigationIntent
import org.harvestcircle.product.ScreenKey

interface IdentityPresentationPort {
    val state: StateFlow<HarvestCirclePresenterState>

    fun dispatch(intent: HarvestCircleIntent)
}

data class HarvestCircleShellState(
    val identity: HarvestCirclePresenterState,
    val buildInfo: BuildInfo,
    val session: ShellSessionState = ShellSessionState(),
    val localUsability: LocalUsability = deriveLocalUsability(identity.snapshot),
    val root: ShellRoot = deriveShellRoot(identity, session),
    val appearance: AppearanceState = AppearanceState(),
    val overlays: OverlayState = OverlayState(),
)

sealed interface HarvestCircleShellIntent {
    data class Identity(
        val intent: HarvestCircleIntent,
    ) : HarvestCircleShellIntent

    data object EnterReadOnly : HarvestCircleShellIntent

    data class Navigate(
        val screenKey: ScreenKey,
    ) : HarvestCircleShellIntent

    data class Navigation(
        val intent: NavigationIntent,
    ) : HarvestCircleShellIntent

    data class Overlay(
        val intent: OverlayIntent,
    ) : HarvestCircleShellIntent

    data class SetTheme(
        val theme: ThemePreference,
    ) : HarvestCircleShellIntent

    data class SetTextSize(
        val textSize: TextSizePreference,
    ) : HarvestCircleShellIntent

    data class SetMotion(
        val motion: MotionPreference,
    ) : HarvestCircleShellIntent
}

class HarvestCircleShellPresenter(
    private val identityPresenter: IdentityPresentationPort,
    buildInfo: BuildInfo,
    scope: CoroutineScope,
    private val referenceParser: NostrReferenceParser = RejectingNostrReferenceParser,
) {
    private val mutableState = MutableStateFlow(HarvestCircleShellState(identityPresenter.state.value, buildInfo))
    val state: StateFlow<HarvestCircleShellState> = mutableState.asStateFlow()
    private val observation: Job =
        scope.launch {
            identityPresenter.state.collect { identity -> updateIdentity(identity) }
        }

    fun dispatch(intent: HarvestCircleShellIntent) {
        when (intent) {
            is HarvestCircleShellIntent.Identity -> identityPresenter.dispatch(intent.intent)
            HarvestCircleShellIntent.EnterReadOnly -> reduce(ShellEvent.EnterReadOnly)
            is HarvestCircleShellIntent.Navigate -> reduce(ShellEvent.Navigate(intent.screenKey))
            is HarvestCircleShellIntent.Navigation -> reduce(ShellEvent.Navigation(intent.intent))
            is HarvestCircleShellIntent.Overlay -> dispatchOverlay(intent.intent)
            is HarvestCircleShellIntent.SetTheme -> reduce(ShellEvent.SetTheme(intent.theme))
            is HarvestCircleShellIntent.SetTextSize -> reduce(ShellEvent.SetTextSize(intent.textSize))
            is HarvestCircleShellIntent.SetMotion -> reduce(ShellEvent.SetMotion(intent.motion))
        }
    }

    fun close() {
        observation.cancel()
    }

    private fun updateIdentity(identity: HarvestCirclePresenterState) {
        reduce(ShellEvent.IdentityObserved(identity))
    }

    private fun dispatchOverlay(intent: OverlayIntent) {
        val confirmation = mutableState.value.overlays.current as? FoundationOverlay.ConfirmAction
        when (intent) {
            is OverlayIntent.EditReference -> {
                if (referenceParser.parse(intent.value).classification == NostrReferenceClassification.PrivateKeyRejected) {
                    applyReferenceResult(ReferenceResult.PrivateKeyRejected, clearInput = true)
                    return
                }
            }
            OverlayIntent.SubmitReference -> {
                val overlay = mutableState.value.overlays.current as? FoundationOverlay.OpenNostrReference ?: return
                val parsed = referenceParser.parse(overlay.input)
                when (parsed.classification) {
                    NostrReferenceClassification.Invalid -> applyReferenceResult(ReferenceResult.Invalid)
                    NostrReferenceClassification.PrivateKeyRejected ->
                        applyReferenceResult(ReferenceResult.PrivateKeyRejected, clearInput = true)
                    NostrReferenceClassification.EventId,
                    NostrReferenceClassification.PublicKey,
                    NostrReferenceClassification.Profile,
                    NostrReferenceClassification.Note,
                    NostrReferenceClassification.Event,
                    NostrReferenceClassification.Address,
                    -> applyReferenceResult(ReferenceResult.Unsupported)
                }
                return
            }
            OverlayIntent.Confirm -> {
                if (mutableState.value.identity.busy) return
                when (confirmation?.action) {
                    ConfirmationAction.RemoveLocalIdentity ->
                        identityPresenter.dispatch(HarvestCircleIntent.ConfirmIdentityRemoval)
                    null -> Unit
                }
            }
            OverlayIntent.Close, OverlayIntent.Escape ->
                if (confirmation?.action == ConfirmationAction.RemoveLocalIdentity) {
                    identityPresenter.dispatch(HarvestCircleIntent.CancelIdentityRemoval)
                }
            else -> Unit
        }
        reduce(ShellEvent.Overlay(intent))
    }

    private fun applyReferenceResult(
        result: ReferenceResult,
        clearInput: Boolean = false,
    ) {
        reduce(ShellEvent.Overlay(OverlayIntent.ApplyReferenceResult(result, clearInput)))
    }

    private fun reduce(event: ShellEvent) {
        mutableState.update { current -> ShellReducer.reduce(current, event) }
    }
}

val HarvestCircleShellState.currentRoute: AppRoute?
    get() = (root as? ShellRoot.Dashboard)?.navigation?.current
