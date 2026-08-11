package org.harvestcircle.application

import org.harvestcircle.ui.shell.SignerStatusLabel
import org.harvestcircle.ui.shell.SyncStatusLabel

sealed interface FoundationOverlay {
    data class ConfirmAction(
        val title: String,
        val explanation: String,
        val actionLabel: String,
        val action: ConfirmationAction,
    ) : FoundationOverlay

    data class SignerStatus(
        val status: SignerStatusLabel,
    ) : FoundationOverlay

    data class SyncStatus(
        val status: SyncStatusLabel,
    ) : FoundationOverlay

    data class OpenNostrReference(
        val input: String = "",
        val result: ReferenceResult? = null,
    ) : FoundationOverlay
}

enum class ConfirmationAction { RemoveLocalIdentity }

enum class BannerSeverity { Information, Caution, Critical }

data class GlobalStatusBanner(
    val message: String,
    val severity: BannerSeverity,
)

data class OverlayState(
    val current: FoundationOverlay? = null,
    val banner: GlobalStatusBanner? = null,
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
            OverlayIntent.SubmitReference -> submitReference(state)
            is OverlayIntent.ApplyReferenceResult -> applyReferenceResult(state, intent.result)
            OverlayIntent.Confirm, OverlayIntent.Close, OverlayIntent.Escape -> state.copy(current = null)
        }

    private fun submitReference(state: OverlayState): OverlayState {
        val overlay = state.current as? FoundationOverlay.OpenNostrReference ?: return state
        return state.copy(current = overlay.copy(result = validateNostrReference(overlay.input)))
    }

    private fun applyReferenceResult(
        state: OverlayState,
        result: ReferenceResult,
    ): OverlayState {
        val overlay = state.current as? FoundationOverlay.OpenNostrReference ?: return state
        return state.copy(current = overlay.copy(result = result))
    }
}

enum class ReferenceResult(
    val message: String,
) {
    Invalid("This reference is not valid."),
    Unsupported("This Nostr reference is not supported by this build."),
}

fun validateNostrReference(raw: String): ReferenceResult {
    if (raw.isBlank() || raw.length > MAX_REFERENCE_CHARS || raw.any(Char::isISOControl)) return ReferenceResult.Invalid
    val value = raw.trim()
    val payload = value.removePrefix("nostr:")
    val accepted =
        payload.matches(Regex("[0-9a-fA-F]{64}")) ||
            payload.matches(Regex("(?:npub|nprofile|note|nevent|naddr)1[023456789acdefghjklmnpqrstuvwxyz]{6,}"))
    return if (accepted && (value == payload || value.startsWith("nostr:"))) {
        ReferenceResult.Unsupported
    } else {
        ReferenceResult.Invalid
    }
}

private const val MAX_REFERENCE_CHARS = 2048
