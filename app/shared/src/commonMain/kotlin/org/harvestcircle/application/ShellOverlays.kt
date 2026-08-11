package org.harvestcircle.application

sealed interface FoundationOverlay {
    data class ConfirmAction(
        val title: String,
        val explanation: String,
        val actionLabel: String,
        val action: ConfirmationAction,
    ) : FoundationOverlay

    data class Status(
        val key: StatusOverlayKey,
    ) : FoundationOverlay

    data class OpenNostrReference(
        val input: String = "",
        val result: ReferenceResult? = null,
    ) : FoundationOverlay
}

enum class ConfirmationAction { RemoveLocalIdentity }

enum class StatusOverlayKey { Signer, Sync }

data class OverlayState(
    val current: FoundationOverlay? = null,
)

sealed interface OverlayIntent {
    data class Open(
        val overlay: FoundationOverlay,
    ) : OverlayIntent

    data class EditReference(
        val value: String,
    ) : OverlayIntent

    data object SubmitReference : OverlayIntent

    data class ApplyReferenceResult(
        val result: ReferenceResult,
        val clearInput: Boolean = false,
    ) : OverlayIntent

    data object Confirm : OverlayIntent

    data object Close : OverlayIntent

    data object Escape : OverlayIntent
}

object OverlayReducer {
    fun reduce(
        state: OverlayState,
        intent: OverlayIntent,
    ): OverlayState =
        when (intent) {
            is OverlayIntent.Open -> state.copy(current = intent.overlay)
            is OverlayIntent.EditReference ->
                state.copy(current = (state.current as? FoundationOverlay.OpenNostrReference)?.copy(input = intent.value))
            OverlayIntent.SubmitReference -> state
            is OverlayIntent.ApplyReferenceResult -> applyReferenceResult(state, intent.result, intent.clearInput)
            OverlayIntent.Confirm, OverlayIntent.Close, OverlayIntent.Escape -> state.copy(current = null)
        }

    private fun applyReferenceResult(
        state: OverlayState,
        result: ReferenceResult,
        clearInput: Boolean,
    ): OverlayState {
        val overlay = state.current as? FoundationOverlay.OpenNostrReference ?: return state
        return state.copy(current = overlay.copy(input = if (clearInput) "" else overlay.input, result = result))
    }
}

enum class ReferenceResult(
    val message: String,
) {
    Invalid("This reference is not valid."),
    PrivateKeyRejected("Private-key references cannot be opened."),
    Unsupported("This Nostr reference is not supported by this build."),
}
