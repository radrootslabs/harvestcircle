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
        val open =
            OverlayState(
                FoundationOverlay.OpenNostrReference(input = "public-reference"),
            )
        val unchanged = OverlayReducer.reduce(open, OverlayIntent.SubmitReference)
        assertEquals(open, unchanged)
        val rejected =
            OverlayReducer.reduce(
                open,
                OverlayIntent.ApplyReferenceResult(ReferenceResult.PrivateKeyRejected, clearInput = true),
            )
        assertEquals(
            FoundationOverlay.OpenNostrReference(input = "", result = ReferenceResult.PrivateKeyRejected),
            rejected.current,
        )
    }

    @Test
    fun oneTopOverlayReplacesPriorAndEscapeDoesNotTouchNavigation() {
        val first =
            OverlayReducer.reduce(
                OverlayState(),
                OverlayIntent.Open(FoundationOverlay.Status(StatusOverlayKey.Signer)),
            )
        val second =
            OverlayReducer.reduce(
                first,
                OverlayIntent.Open(FoundationOverlay.Status(StatusOverlayKey.Sync)),
            )
        assertEquals(FoundationOverlay.Status(StatusOverlayKey.Sync), second.current)
        assertNull(OverlayReducer.reduce(second, OverlayIntent.Escape).current)
    }

    @Test
    fun deferredOverlaysHaveNoFoundationStateConstructor() {
        val deferred = HarvestCircleSurfaceRegistry.overlays.filter { it.availability.name.startsWith("Deferred") }
        assertEquals(5, deferred.size)
        assertTrue(FeatureAvailability.Foundation in HarvestCircleSurfaceRegistry.overlays.map { it.availability })
    }
}
