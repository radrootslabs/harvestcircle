package org.harvestcircle.application

import org.harvestcircle.product.FeatureAvailability
import org.harvestcircle.product.HarvestCircleSurfaceRegistry
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ShellOverlaysTest {
    @Test
    fun typedReferenceResultsAreAppliedWithoutParsingInTheReducer() {
        val open = shellState(FoundationOverlay.OpenNostrReference(input = "public-reference"))
        val unchanged = OverlayReducer.transition(open, OverlayIntent.SubmitReference).state
        assertEquals(open, unchanged)
        val rejected =
            OverlayReducer
                .transition(
                    open,
                    OverlayIntent.ApplyReferenceResult(ReferenceResult.PrivateKeyRejected, clearInput = true),
                ).state
        assertEquals(
            FoundationOverlay.OpenNostrReference(input = "", result = ReferenceResult.PrivateKeyRejected),
            rejected.overlays.current,
        )
    }

    @Test
    fun hcSl001AmbiguousHexEditClearsInputWithTypedResult() {
        val hex = "01".repeat(32)
        val open = shellState(FoundationOverlay.OpenNostrReference())
        val edited = OverlayReducer.transition(open, OverlayIntent.EditReference(" nostr:$hex\n")).state

        assertEquals(
            FoundationOverlay.OpenNostrReference(input = "", result = ReferenceResult.AmbiguousHex),
            edited.overlays.current,
        )
        assertTrue(hex !in edited.toString())
    }

    @Test
    fun hcSc012ReferenceOpeningIsInputFreeAndGenericPrefilledIngressIsRejected() {
        val initial = shellState()
        val rejected =
            OverlayReducer
                .transition(
                    initial,
                    OverlayIntent.Open(FoundationOverlay.OpenNostrReference("nsec1prefilled")),
                ).state
        assertEquals(initial, rejected)

        val opened =
            OverlayReducer
                .transition(initial, OverlayIntent.OpenReference(ShellFocusTarget.TodayReference))
                .state
        assertEquals(FoundationOverlay.OpenNostrReference(), opened.overlays.current)
        assertEquals(ShellFocusTarget.TodayReference, opened.overlays.returnFocus)

        val closed = OverlayReducer.transition(opened, OverlayIntent.Close).state
        assertNull(closed.overlays.current)
        assertEquals(ShellFocusTarget.TodayReference, closed.overlays.restoreFocus)
    }

    @Test
    fun oneTopOverlayReplacesPriorAndEscapeDoesNotTouchNavigation() {
        val first =
            OverlayReducer
                .transition(
                    shellState(),
                    OverlayIntent.Open(FoundationOverlay.Status(StatusOverlayKey.Signer)),
                ).state
        val second =
            OverlayReducer
                .transition(
                    first,
                    OverlayIntent.Open(FoundationOverlay.Status(StatusOverlayKey.Sync)),
                ).state
        assertEquals(FoundationOverlay.Status(StatusOverlayKey.Sync), second.overlays.current)
        assertNull(
            OverlayReducer
                .transition(second, OverlayIntent.Escape())
                .state.overlays.current,
        )
    }

    @Test
    fun hcSl006UnrelatedIdentityBusyDoesNotBlockReadyConfirmationAdmission() {
        val identityId = IdentityId.fromPublicKeyHex("02".repeat(32))
        val requestId = RemovalRequestId.from("overlay-local-busy")
        val action = ConfirmationAction.RemoveLocalIdentity(identityId, requestId)
        val confirmation =
            IdentityRemovalConfirmation(
                identityId = identityId,
                requestId = requestId,
                deletesLocalCredential = true,
                signsOut = false,
                expiresAt = UnixSeconds(60),
            )
        val initial =
            shellState(
                FoundationOverlay.ConfirmAction(
                    title = "Remove this saved identity?",
                    explanation = "Its local credential will be deleted.",
                    actionLabel = "Remove local identity",
                    action = action,
                ),
            )
        val busy =
            initial.copy(
                identity =
                    initial.identity.copy(
                        busy = true,
                        removalConfirmation = confirmation,
                        removalStatus = RemovalStatus.AWAITING_CONFIRMATION,
                    ),
            )

        val transition = OverlayReducer.transition(busy, OverlayIntent.Confirm(action))

        assertEquals(
            ConfirmationPhase.Submitting,
            (transition.state.overlays.current as FoundationOverlay.ConfirmAction).phase,
        )
        assertEquals(
            listOf(ShellEffect.DispatchIdentity(HarvestCircleIntent.ConfirmIdentityRemoval(identityId, requestId))),
            transition.effects,
        )
    }

    @Test
    fun deferredOverlaysHaveNoFoundationStateConstructor() {
        val deferred = HarvestCircleSurfaceRegistry.overlays.filter { it.availability.name.startsWith("Deferred") }
        assertEquals(5, deferred.size)
        assertTrue(FeatureAvailability.Foundation in HarvestCircleSurfaceRegistry.overlays.map { it.availability })
    }
}

private fun shellState(overlay: FoundationOverlay? = null): HarvestCircleShellState =
    HarvestCircleShellState(
        identity =
            HarvestCirclePresenterState(
                ApplicationSnapshot(
                    revision = SnapshotRevision(1UL),
                    lifecycle = ApplicationLifecycle.Ready,
                    lifecycleProblem = null,
                    configuredRelays = emptyList(),
                    identities = emptyList(),
                    selectedIdentityId = null,
                    session = SessionLifecycle.SignedOut,
                    sessionSubjectIdentityId = null,
                    sessionProblem = null,
                    activeIdentity = null,
                    recoverableProblem = null,
                ),
            ),
        buildInfo = BuildInfo.unknown(),
        overlays = OverlayState(overlay),
    )
