package org.harvestcircle.application

sealed interface FoundationOverlay {
    data class ConfirmAction(
        val title: String,
        val explanation: String,
        val actionLabel: String,
        val action: ConfirmationAction,
        val phase: ConfirmationPhase = ConfirmationPhase.Ready,
    ) : FoundationOverlay {
        val busy: Boolean
            get() = phase != ConfirmationPhase.Ready
    }

    data class Status(
        val key: StatusOverlayKey,
    ) : FoundationOverlay

    data class OpenNostrReference(
        val input: String = "",
        val result: ReferenceResult? = null,
    ) : FoundationOverlay
}

sealed interface ConfirmationAction {
    data class RemoveLocalIdentity(
        val identityId: IdentityId,
        val requestId: RemovalRequestId,
    ) : ConfirmationAction
}

enum class ConfirmationPhase { Ready, Submitting, Dismissing }

enum class StatusOverlayKey { Signer, Sync }

data class OverlayState(
    val current: FoundationOverlay? = null,
    val returnFocus: ShellFocusTarget? = null,
    val restoreFocus: ShellFocusTarget? = null,
)

sealed interface ShellFocusTarget {
    data object RouteFallback : ShellFocusTarget

    data object BootstrapFallback : ShellFocusTarget

    data object TodayReference : ShellFocusTarget

    data object TopBarReference : ShellFocusTarget

    data object TopBarSync : ShellFocusTarget

    data object TopBarSigner : ShellFocusTarget

    data class IdentityRow(
        val publicKeyHex: String,
    ) : ShellFocusTarget
}

sealed interface OverlayIntent {
    data class Open(
        val overlay: FoundationOverlay,
        val returnFocus: ShellFocusTarget = ShellFocusTarget.RouteFallback,
    ) : OverlayIntent

    data class OpenReference(
        val returnFocus: ShellFocusTarget = ShellFocusTarget.RouteFallback,
    ) : OverlayIntent

    class EditReference(
        val value: String,
    ) : OverlayIntent {
        override fun toString(): String = "EditReference(value=[REDACTED])"
    }

    data object SubmitReference : OverlayIntent

    data class ApplyReferenceResult(
        val result: ReferenceResult,
        val clearInput: Boolean = false,
    ) : OverlayIntent

    data class Confirm(
        val action: ConfirmationAction,
    ) : OverlayIntent

    data class DismissConfirmation(
        val action: ConfirmationAction,
    ) : OverlayIntent

    data object Close : OverlayIntent

    data class Escape(
        val confirmation: ConfirmationAction? = null,
    ) : OverlayIntent
}

sealed interface ShellEffect {
    data class DispatchIdentity(
        val intent: HarvestCircleIntent,
    ) : ShellEffect
}

data class OverlayTransition(
    val state: HarvestCircleShellState,
    val effects: List<ShellEffect> = emptyList(),
)

object OverlayReducer {
    fun transition(
        state: HarvestCircleShellState,
        intent: OverlayIntent,
    ): OverlayTransition =
        when (intent) {
            is OverlayIntent.Open ->
                if (intent.overlay is FoundationOverlay.OpenNostrReference) {
                    OverlayTransition(state)
                } else {
                    state.openOverlay(intent.overlay, intent.returnFocus)
                }
            is OverlayIntent.OpenReference ->
                state.openOverlay(FoundationOverlay.OpenNostrReference(), intent.returnFocus)
            is OverlayIntent.EditReference -> applyReferenceEdit(state, intent.value)
            OverlayIntent.SubmitReference -> OverlayTransition(state)
            is OverlayIntent.ApplyReferenceResult -> applyReferenceResult(state, intent.result, intent.clearInput)
            is OverlayIntent.Confirm -> admitConfirmation(state, intent.action, submitting = true)
            is OverlayIntent.DismissConfirmation -> admitConfirmation(state, intent.action, submitting = false)
            OverlayIntent.Close ->
                if (state.overlays.current is FoundationOverlay.ConfirmAction) {
                    OverlayTransition(state)
                } else {
                    state.closeOverlay()
                }
            is OverlayIntent.Escape ->
                when (val current = state.overlays.current) {
                    is FoundationOverlay.ConfirmAction -> {
                        val expected = intent.confirmation
                        if (expected == null) OverlayTransition(state) else admitConfirmation(state, expected, submitting = false)
                    }
                    null -> OverlayTransition(state)
                    else -> state.closeOverlay()
                }
        }

    private fun applyReferenceEdit(
        state: HarvestCircleShellState,
        raw: String,
    ): OverlayTransition {
        val overlay = state.overlays.current as? FoundationOverlay.OpenNostrReference ?: return OverlayTransition(state)
        val updated =
            when (val admission = ReferenceInputPolicy.admit(raw)) {
                is ReferenceInputAdmission.Accepted -> overlay.copy(input = admission.value, result = null)
                ReferenceInputAdmission.PrivateKeyShaped -> overlay.copy(input = "", result = ReferenceResult.PrivateKeyRejected)
                ReferenceInputAdmission.TooLarge -> overlay.copy(input = "", result = ReferenceResult.Invalid)
                ReferenceInputAdmission.AmbiguousHex -> overlay.copy(input = "", result = ReferenceResult.AmbiguousHex)
            }
        return state.updateOverlay(updated)
    }

    private fun admitConfirmation(
        state: HarvestCircleShellState,
        expected: ConfirmationAction,
        submitting: Boolean,
    ): OverlayTransition {
        val current = state.overlays.current as? FoundationOverlay.ConfirmAction ?: return OverlayTransition(state)
        val removal = expected as? ConfirmationAction.RemoveLocalIdentity ?: return OverlayTransition(state)
        val admitted = state.identity.removalConfirmation ?: return OverlayTransition(state)
        if (current.action != expected ||
            current.phase != ConfirmationPhase.Ready ||
            state.identity.removalStatus != RemovalStatus.AWAITING_CONFIRMATION ||
            admitted.identityId != removal.identityId ||
            admitted.requestId != removal.requestId
        ) {
            return OverlayTransition(state)
        }
        val phase = if (submitting) ConfirmationPhase.Submitting else ConfirmationPhase.Dismissing
        val identityIntent =
            if (submitting) {
                HarvestCircleIntent.ConfirmIdentityRemoval(removal.identityId, removal.requestId)
            } else {
                HarvestCircleIntent.CancelIdentityRemoval(removal.identityId, removal.requestId)
            }
        return OverlayTransition(
            state.copy(overlays = state.overlays.copy(current = current.copy(phase = phase))),
            listOf(ShellEffect.DispatchIdentity(identityIntent)),
        )
    }

    private fun applyReferenceResult(
        state: HarvestCircleShellState,
        result: ReferenceResult,
        clearInput: Boolean,
    ): OverlayTransition {
        val overlay = state.overlays.current as? FoundationOverlay.OpenNostrReference ?: return OverlayTransition(state)
        return state.updateOverlay(overlay.copy(input = if (clearInput) "" else overlay.input, result = result))
    }

    private fun HarvestCircleShellState.openOverlay(
        overlay: FoundationOverlay,
        returnFocus: ShellFocusTarget,
    ): OverlayTransition = OverlayTransition(copy(overlays = overlays.opened(overlay, returnFocus)))

    private fun HarvestCircleShellState.updateOverlay(overlay: FoundationOverlay): OverlayTransition =
        OverlayTransition(copy(overlays = overlays.copy(current = overlay)))

    private fun HarvestCircleShellState.closeOverlay(): OverlayTransition = OverlayTransition(copy(overlays = overlays.closed()))
}

internal fun OverlayState.opened(
    overlay: FoundationOverlay,
    returnFocus: ShellFocusTarget,
): OverlayState = copy(current = overlay, returnFocus = returnFocus, restoreFocus = null)

internal fun OverlayState.closed(): OverlayState = copy(current = null, returnFocus = null, restoreFocus = returnFocus)

enum class ReferenceResult(
    val message: String,
) {
    Invalid("This reference is not valid."),
    PrivateKeyRejected("Private-key references cannot be opened."),
    AmbiguousHex(
        "Bare hexadecimal references are not accepted because they can also be private keys.\n\n" +
            "Use a note1 or nevent1 reference.",
    ),
    Unsupported("This Nostr reference is not supported by this build."),
}
