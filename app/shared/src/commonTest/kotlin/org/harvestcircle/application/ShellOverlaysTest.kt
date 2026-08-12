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
    fun referenceOpeningIsInputFreeAndGenericPrefilledIngressIsRejected() {
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
