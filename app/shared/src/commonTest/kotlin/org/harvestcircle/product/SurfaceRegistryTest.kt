package org.harvestcircle.product

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class SurfaceRegistryTest {
    @Test
    fun registryHasExactStableExternalKeys() {
        assertEquals(expectedScreenKeys, HarvestCircleSurfaceRegistry.screens.map(ScreenDescriptor::externalKey).toSet())
        assertEquals(expectedOverlayKeys, HarvestCircleSurfaceRegistry.overlays.map(OverlayDescriptor::externalKey).toSet())
    }

    @Test
    fun keysAreUniqueLowerSnakeCase() {
        val keys =
            HarvestCircleSurfaceRegistry.screens.map(ScreenDescriptor::externalKey) +
                HarvestCircleSurfaceRegistry.overlays.map(OverlayDescriptor::externalKey)
        assertEquals(keys.size, keys.toSet().size)
        assertTrue(keys.all { it.matches(Regex("[a-z][a-z0-9]*(?:_[a-z0-9]+)*")) })
    }

    @Test
    fun foundationClassificationIsCapabilityBounded() {
        val executable = ScreenKey.entries.filter { it.descriptor.availability == FeatureAvailability.Foundation }
        assertEquals(setOf(ScreenKey.Bootstrap, ScreenKey.PersonalToday, ScreenKey.Network, ScreenKey.Settings), executable.toSet())
        assertEquals(LayoutKind.Canvas, ScreenKey.Bootstrap.descriptor.layout)
        assertTrue(executable.filterNot { it == ScreenKey.Bootstrap }.all { it.descriptor.layout == LayoutKind.Dashboard })
        assertEquals(FeatureAvailability.FoundationSyntaxOnly, OverlayKey.OpenNostrReference.descriptor.availability)
    }
}

private val expectedScreenKeys =
    setOf(
        "bootstrap_screen",
        "signer_connection_screen",
        "startup_recovery_screen",
        "runtime_recovery_screen",
        "protocol_compatibility_screen",
        "signing_review_screen",
        "proof_chain_screen",
        "fulfillment_issue_screen",
        "personal_today_screen",
        "explore_screen",
        "farm_profile_screen",
        "circle_screen",
        "activity_screen",
        "commitment_screen",
        "allocation_screen",
        "farm_overview_screen",
        "round_studio_screen",
        "live_round_screen",
        "pickup_desk_screen",
        "round_outcome_screen",
        "network_screen",
        "settings_screen",
    )

private val expectedOverlayKeys =
    setOf(
        "proof_inspector_panel",
        "open_nostr_reference_dialog",
        "relay_editor_dialog",
        "authority_selector_dialog",
        "locality_editor_dialog",
        "share_round_dialog",
        "confirm_action_dialog",
        "signer_status_popover",
        "sync_status_popover",
        "global_status_banner",
    )
