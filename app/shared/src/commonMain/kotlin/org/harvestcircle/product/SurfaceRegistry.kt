package org.harvestcircle.product

enum class LayoutKind { Dashboard, Canvas }

enum class WorkspaceKind { System, Personal, Farm, Shared }

enum class NavigationKind { Permanent, Contextual, Workflow }

enum class FeatureAvailability {
    Foundation,
    FoundationPartial,
    FoundationSyntaxOnly,
    DeferredSigner,
    DeferredProtocol,
    DeferredCollective,
    DeferredNetworkPersistence,
    DeferredLocality,
}

data class ScreenDescriptor(
    val externalKey: String,
    val layout: LayoutKind,
    val workspace: WorkspaceKind,
    val navigation: NavigationKind,
    val availability: FeatureAvailability,
)

data class OverlayDescriptor(
    val externalKey: String,
    val availability: FeatureAvailability,
)

enum class ScreenKey(
    val descriptor: ScreenDescriptor,
) {
    Bootstrap(
        screen(
            "bootstrap_screen",
            LayoutKind.Canvas,
            WorkspaceKind.System,
            NavigationKind.Workflow,
            FeatureAvailability.Foundation,
        ),
    ),
    SignerConnection(
        screen(
            "signer_connection_screen",
            LayoutKind.Canvas,
            WorkspaceKind.System,
            NavigationKind.Workflow,
            FeatureAvailability.DeferredSigner,
        ),
    ),
    StartupRecovery(
        screen(
            "startup_recovery_screen",
            LayoutKind.Canvas,
            WorkspaceKind.System,
            NavigationKind.Workflow,
            FeatureAvailability.FoundationPartial,
        ),
    ),
    RuntimeRecovery(
        screen(
            "runtime_recovery_screen",
            LayoutKind.Canvas,
            WorkspaceKind.System,
            NavigationKind.Workflow,
            FeatureAvailability.FoundationPartial,
        ),
    ),
    ProtocolCompatibility(
        screen(
            "protocol_compatibility_screen",
            LayoutKind.Canvas,
            WorkspaceKind.System,
            NavigationKind.Workflow,
            FeatureAvailability.FoundationPartial,
        ),
    ),
    SigningReview(
        screen(
            "signing_review_screen",
            LayoutKind.Canvas,
            WorkspaceKind.System,
            NavigationKind.Workflow,
            FeatureAvailability.DeferredProtocol,
        ),
    ),
    ProofChain(
        screen(
            "proof_chain_screen",
            LayoutKind.Canvas,
            WorkspaceKind.System,
            NavigationKind.Workflow,
            FeatureAvailability.DeferredProtocol,
        ),
    ),
    FulfillmentIssue(
        screen(
            "fulfillment_issue_screen",
            LayoutKind.Canvas,
            WorkspaceKind.System,
            NavigationKind.Workflow,
            FeatureAvailability.DeferredProtocol,
        ),
    ),
    PersonalToday(
        screen(
            "personal_today_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Personal,
            NavigationKind.Permanent,
            FeatureAvailability.Foundation,
        ),
    ),
    Explore(
        screen(
            "explore_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Personal,
            NavigationKind.Permanent,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    FarmProfile(
        screen(
            "farm_profile_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Personal,
            NavigationKind.Contextual,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    Circle(
        screen(
            "circle_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Personal,
            NavigationKind.Contextual,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    Activity(
        screen(
            "activity_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Personal,
            NavigationKind.Permanent,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    Commitment(
        screen(
            "commitment_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Personal,
            NavigationKind.Contextual,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    Allocation(
        screen(
            "allocation_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Personal,
            NavigationKind.Contextual,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    FarmOverview(
        screen(
            "farm_overview_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Farm,
            NavigationKind.Permanent,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    RoundBuilder(
        screen(
            "round_builder_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Farm,
            NavigationKind.Permanent,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    LiveRound(
        screen(
            "live_round_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Farm,
            NavigationKind.Contextual,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    PickupDesk(
        screen(
            "pickup_desk_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Farm,
            NavigationKind.Permanent,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    RoundOutcome(
        screen(
            "round_outcome_screen",
            LayoutKind.Dashboard,
            WorkspaceKind.Farm,
            NavigationKind.Contextual,
            FeatureAvailability.DeferredCollective,
        ),
    ),
    Network(screen("network_screen", LayoutKind.Dashboard, WorkspaceKind.Shared, NavigationKind.Permanent, FeatureAvailability.Foundation)),
    Settings(
        screen("settings_screen", LayoutKind.Dashboard, WorkspaceKind.Shared, NavigationKind.Contextual, FeatureAvailability.Foundation),
    ),
}

enum class OverlayKey(
    val descriptor: OverlayDescriptor,
) {
    ProofInspector(overlay("proof_inspector_panel", FeatureAvailability.DeferredProtocol)),
    OpenNostrReference(overlay("open_nostr_reference_dialog", FeatureAvailability.FoundationSyntaxOnly)),
    RelayEditor(overlay("relay_editor_dialog", FeatureAvailability.DeferredNetworkPersistence)),
    AuthoritySelector(overlay("authority_selector_dialog", FeatureAvailability.DeferredCollective)),
    LocalityEditor(overlay("locality_editor_dialog", FeatureAvailability.DeferredLocality)),
    ShareRound(overlay("share_round_dialog", FeatureAvailability.DeferredCollective)),
    ConfirmAction(overlay("confirm_action_dialog", FeatureAvailability.Foundation)),
    SignerStatus(overlay("signer_status_popover", FeatureAvailability.Foundation)),
    SyncStatus(overlay("sync_status_popover", FeatureAvailability.Foundation)),
    GlobalStatusBanner(overlay("global_status_banner", FeatureAvailability.Foundation)),
}

object HarvestCircleSurfaceRegistry {
    val screens: List<ScreenDescriptor> = ScreenKey.entries.map(ScreenKey::descriptor)
    val overlays: List<OverlayDescriptor> = OverlayKey.entries.map(OverlayKey::descriptor)
}

private fun screen(
    externalKey: String,
    layout: LayoutKind,
    workspace: WorkspaceKind,
    navigation: NavigationKind,
    availability: FeatureAvailability,
): ScreenDescriptor = ScreenDescriptor(externalKey, layout, workspace, navigation, availability)

private fun overlay(
    externalKey: String,
    availability: FeatureAvailability,
): OverlayDescriptor = OverlayDescriptor(externalKey, availability)
