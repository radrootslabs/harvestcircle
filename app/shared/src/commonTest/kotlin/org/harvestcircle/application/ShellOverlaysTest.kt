package org.harvestcircle.application

import org.harvestcircle.product.FeatureAvailability
import org.harvestcircle.product.HarvestCircleSurfaceRegistry
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ShellOverlaysTest {
    @Test
    fun acceptedReferencesRemainSyntaxOnlyAndMalformedInputFails() {
        assertEquals(ReferenceResult.Unsupported, validateNostrReference("ab".repeat(32)))
        assertEquals(ReferenceResult.Unsupported, validateNostrReference("nostr:note1qqqqqq"))
        assertEquals(ReferenceResult.Invalid, validateNostrReference("nostr:bad reference"))
        assertEquals(ReferenceResult.Invalid, validateNostrReference("note1abc\u0000"))
    }

    @Test
    fun oneTopOverlayReplacesPriorAndEscapeDoesNotTouchNavigation() {
        val first =
            OverlayReducer.reduce(
                OverlayState(),
                OverlayIntent.Open(FoundationOverlay.SignerStatus(org.harvestcircle.ui.shell.SignerStatusLabel.Available)),
            )
        val second =
            OverlayReducer.reduce(
                first,
                OverlayIntent.Open(FoundationOverlay.SyncStatus(org.harvestcircle.ui.shell.SyncStatusLabel.Degraded)),
            )
        assertTrue(second.current is FoundationOverlay.SyncStatus)
        assertNull(OverlayReducer.reduce(second, OverlayIntent.Escape).current)
    }

    @Test
    fun deferredOverlaysHaveNoFoundationStateConstructor() {
        val deferred = HarvestCircleSurfaceRegistry.overlays.filter { it.availability.name.startsWith("Deferred") }
        assertEquals(5, deferred.size)
        assertTrue(FeatureAvailability.Foundation in HarvestCircleSurfaceRegistry.overlays.map { it.availability })
    }
}
