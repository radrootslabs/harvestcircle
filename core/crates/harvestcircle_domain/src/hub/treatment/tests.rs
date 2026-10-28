use super::*;
use crate::hub::{
    HubId, HubRevision,
    portions::{PhysicalLineage, PhysicalLineageNode, PhysicalScopeAnchor},
    reference::{
        HubLabel, ReferenceCatalog, ReferenceValue, SampleHandlingBasis, SourceProvenance,
        WholePacks,
    },
    stock::Occurrence,
};
use crate::{PublicKey, UnixTimestamp};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn actor() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn node(n: u8, parent: Option<u8>, born: u64, count: u32) -> PhysicalLineageNode {
    PhysicalLineageNode {
        id: id(n),
        parent: parent.map(id),
        trace: id(100),
        variant: id(2),
        created_revision: rev(born),
        packs: packs(count),
    }
}
fn graph(revision: u64, nodes: Vec<PhysicalLineageNode>) -> PhysicalLineage {
    PhysicalLineage::new(
        HubId::from_bytes([1; 16]).unwrap(),
        id(100),
        id(2),
        rev(revision),
        nodes,
    )
    .unwrap()
}
fn catalog() -> ReferenceCatalog {
    let mut catalog = ReferenceCatalog::default();
    catalog
        .create(
            catalog.revision(),
            id(1),
            ReferenceValue::Product {
                label: HubLabel::new("Sample apples").unwrap(),
            },
        )
        .unwrap();
    catalog
        .create(
            catalog.revision(),
            id(2),
            ReferenceValue::Pack {
                product: id(1),
                label: HubLabel::new("Original crate").unwrap(),
                nominal_grams: None,
            },
        )
        .unwrap();
    catalog
}
fn selection(ids: &[u8]) -> TreatmentSelection {
    TreatmentSelection::new(ids.iter().copied().map(id).collect()).unwrap()
}
fn command(graph: &PhysicalLineage, action: TreatmentAction) -> TreatmentCommand {
    TreatmentCommand {
        expected_stock_revision: graph.revision(),
        expected_catalog_revision: catalog().revision(),
        action,
        occurred: Occurrence::Unknown,
        observation: SourceProvenance::new("Explicit sample decision from manual observation")
            .unwrap(),
    }
}
fn accept(ids: &[u8], status: AcceptanceStatus) -> TreatmentAction {
    TreatmentAction::SetAcceptance {
        selection: selection(ids),
        status,
    }
}
fn hold(ids: &[u8]) -> TreatmentAction {
    TreatmentAction::PlaceHold {
        selection: selection(ids),
    }
}
fn release(n: u8) -> TreatmentAction {
    TreatmentAction::ReleaseHold {
        hold: id(n),
        expected_revision: HubRevision::INITIAL,
    }
}

fn apply(
    state: &mut TreatmentState,
    graph: &mut PhysicalLineage,
    n: u8,
    action: TreatmentAction,
) -> TreatmentPlan {
    let plan = command(graph, action)
        .plan(&catalog(), graph, state, id(n), actor(), at(100))
        .unwrap();
    match &plan.change {
        TreatmentChange::Acceptance(value) => state.decisions.push(*value.clone()),
        TreatmentChange::HoldPlaced(value) => state.holds.push(*value.clone()),
        TreatmentChange::HoldReleased { hold, release } => {
            state
                .holds
                .iter_mut()
                .find(|item| item.stamp.id == *hold)
                .unwrap()
                .release = Some(*release.clone())
        }
    }
    *graph = PhysicalLineage::new(
        graph.hub(),
        graph.trace(),
        graph.variant(),
        plan.revision,
        graph.nodes().to_vec(),
    )
    .unwrap();
    assert_eq!(
        state
            .project(graph, SampleHandlingBasis::WHOLE_PACK_V1)
            .unwrap()
            .summary,
        plan.summary
    );
    plan
}

#[test]
fn default_review_and_rejection_preserve_custody_and_internal_treatment() {
    let mut graph = graph(2, vec![node(10, None, 2, 20)]);
    let mut state = TreatmentState::default();
    let initial = state
        .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
        .unwrap();
    assert_eq!(initial.summary.review_required, packs(20));
    assert_eq!(initial.summary.departure_eligible, packs(0));
    assert_eq!(
        initial.require(&selection(&[10]), PhysicalAction::NormalDeparture),
        Err(TreatmentError::ActionBlocked)
    );
    initial
        .require(&selection(&[10]), PhysicalAction::InternalRelocation)
        .unwrap();
    let rejected = apply(
        &mut state,
        &mut graph,
        30,
        accept(&[10], AcceptanceStatus::Rejected),
    );
    assert_eq!(rejected.summary.on_hand, packs(20));
    assert_eq!(rejected.summary.rejected, packs(20));
    assert_eq!(rejected.summary.departure_eligible, packs(0));
    for action in [
        PhysicalAction::InternalRelocation,
        PhysicalAction::ObservedSeparation,
        PhysicalAction::Disposition,
    ] {
        state
            .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
            .unwrap()
            .require(&selection(&[10]), action)
            .unwrap();
    }
}

#[test]
fn fx05_overlapping_holds_restrict_eight_and_selected_release_preserves_other_hold() {
    let mut graph = graph(2, vec![node(10, None, 2, 20)]);
    let mut state = TreatmentState::default();
    apply(
        &mut state,
        &mut graph,
        30,
        accept(&[10], AcceptanceStatus::Accepted),
    );
    graph = super::tests::graph(
        5,
        vec![
            node(10, None, 2, 12),
            node(11, Some(10), 4, 3),
            node(12, Some(11), 5, 5),
        ],
    );
    apply(&mut state, &mut graph, 31, hold(&[12]));
    let both = apply(&mut state, &mut graph, 32, hold(&[11, 12]));
    assert_eq!(
        (
            both.summary.on_hand,
            both.summary.restricted,
            both.summary.departure_eligible
        ),
        (packs(20), packs(8), packs(12))
    );
    let a = apply(&mut state, &mut graph, 33, release(31));
    assert_eq!(
        (a.summary.restricted, a.summary.departure_eligible),
        (packs(8), packs(12))
    );
    let current = state
        .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
        .unwrap();
    assert_eq!(
        current
            .groups
            .iter()
            .find(|row| row.group == id(12))
            .unwrap()
            .active_holds,
        vec![id(32)]
    );
    let b = apply(&mut state, &mut graph, 34, release(32));
    assert_eq!(
        (
            b.summary.restricted,
            b.summary.departure_eligible,
            b.summary.on_hand
        ),
        (packs(0), packs(20), packs(20))
    );
    assert_eq!(state.holds()[0].revision(), rev(2));
    assert_eq!(state.holds()[0].stamp.id, id(31));
}

#[test]
fn release_never_supplies_acceptance_and_acceptance_never_releases_hold() {
    let mut graph = graph(2, vec![node(10, None, 2, 20)]);
    let mut state = TreatmentState::default();
    apply(&mut state, &mut graph, 30, hold(&[10]));
    let cleared = apply(&mut state, &mut graph, 31, release(30));
    assert_eq!(
        (
            cleared.summary.restricted,
            cleared.summary.review_required,
            cleared.summary.departure_eligible
        ),
        (packs(0), packs(20), packs(0))
    );
    apply(&mut state, &mut graph, 32, hold(&[10]));
    let accepted = apply(
        &mut state,
        &mut graph,
        33,
        accept(&[10], AcceptanceStatus::Accepted),
    );
    assert_eq!(
        (
            accepted.summary.accepted,
            accepted.summary.restricted,
            accepted.summary.departure_eligible
        ),
        (packs(20), packs(20), packs(0))
    );
    let current = state
        .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
        .unwrap();
    current
        .require(&selection(&[10]), PhysicalAction::InternalRelocation)
        .unwrap();
    assert_eq!(
        current.require(&selection(&[10]), PhysicalAction::NormalDeparture),
        Err(TreatmentError::ActionBlocked)
    );
}

#[test]
fn hold_inheritance_respects_capture_cut_and_never_reenters_excluded_branch() {
    let mut graph = graph(2, vec![node(10, None, 2, 20)]);
    let mut state = TreatmentState::default();
    apply(
        &mut state,
        &mut graph,
        30,
        accept(&[10], AcceptanceStatus::Accepted),
    );
    graph = super::tests::graph(4, vec![node(10, None, 2, 12), node(11, Some(10), 4, 8)]);
    apply(&mut state, &mut graph, 31, hold(&[10]));
    graph = super::tests::graph(
        6,
        vec![
            node(10, None, 2, 12),
            node(11, Some(10), 4, 3),
            node(12, Some(11), 6, 5),
        ],
    );
    let projection = state
        .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
        .unwrap();
    assert_eq!(
        (
            projection.summary.restricted,
            projection.summary.departure_eligible
        ),
        (packs(12), packs(8))
    );
    apply(&mut state, &mut graph, 32, hold(&[11]));
    graph = super::tests::graph(
        8,
        vec![
            node(10, None, 2, 10),
            node(13, Some(10), 8, 2),
            node(11, Some(10), 4, 3),
            node(12, Some(11), 6, 5),
        ],
    );
    let projection = state
        .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
        .unwrap();
    assert_eq!(
        (
            projection.summary.restricted,
            projection.summary.departure_eligible
        ),
        (packs(15), packs(5))
    );
    assert_eq!(
        projection
            .groups
            .iter()
            .find(|row| row.group == id(13))
            .unwrap()
            .active_holds,
        vec![id(31)]
    );
    assert!(
        projection
            .groups
            .iter()
            .find(|row| row.group == id(12))
            .unwrap()
            .active_holds
            .is_empty()
    );
}

#[test]
fn most_recent_recorded_acceptance_wins_without_turning_record_time_into_actual_time() {
    let mut graph = graph(2, vec![node(10, None, 2, 20)]);
    let mut state = TreatmentState::default();
    apply(
        &mut state,
        &mut graph,
        30,
        accept(&[10], AcceptanceStatus::Accepted),
    );
    graph = super::tests::graph(4, vec![node(10, None, 2, 12), node(11, Some(10), 4, 8)]);
    apply(
        &mut state,
        &mut graph,
        31,
        accept(&[11], AcceptanceStatus::Rejected),
    );
    state.decisions[1].stamp.recorded_at = at(90);
    graph = super::tests::graph(
        6,
        vec![
            node(10, None, 2, 12),
            node(11, Some(10), 4, 3),
            node(12, Some(11), 6, 5),
        ],
    );
    let projection = state
        .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
        .unwrap();
    assert_eq!(
        (
            projection.summary.accepted,
            projection.summary.rejected,
            projection.summary.departure_eligible
        ),
        (packs(12), packs(8), packs(12))
    );
    state.decisions.reverse();
    assert_eq!(
        projection,
        state
            .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
            .unwrap()
    );
}

#[test]
fn native_scope_restoration_keeps_empty_ancestors_and_rejects_history_gaps() {
    let initial = graph(2, vec![node(10, None, 2, 20)]);
    let scope = initial.capture(&[id(10)]).unwrap();
    let empty = graph(9, vec![node(10, None, 2, 0)]);
    let restored = empty
        .restore_scope(scope.captured_revision(), scope.anchors().to_vec())
        .unwrap();
    assert_eq!(empty.resolve(&restored).unwrap().packs(), packs(0));
    for anchor in [
        PhysicalScopeAnchor {
            group: id(99),
            ..scope.anchors()[0]
        },
        PhysicalScopeAnchor {
            created_revision: rev(3),
            ..scope.anchors()[0]
        },
        PhysicalScopeAnchor {
            captured_packs: packs(0),
            ..scope.anchors()[0]
        },
    ] {
        assert!(empty.restore_scope(rev(2), vec![anchor]).is_err());
    }
    assert!(
        empty
            .restore_scope(rev(1), scope.anchors().to_vec())
            .is_err()
    );
    assert!(
        empty
            .restore_scope(rev(10), scope.anchors().to_vec())
            .is_err()
    );
    assert!(
        empty
            .restore_scope(rev(2), vec![scope.anchors()[0]; 2])
            .is_err()
    );
}

#[test]
fn stale_versions_closed_hold_and_invalid_effective_time_cannot_change_state() {
    let mut graph = graph(2, vec![node(10, None, 2, 20)]);
    let mut state = TreatmentState::default();
    let mut stale = command(&graph, hold(&[10]));
    stale.expected_catalog_revision = rev(99);
    assert_eq!(
        stale.plan(&catalog(), &graph, &state, id(30), actor(), at(100)),
        Err(TreatmentError::StaleRevision)
    );
    let original = command(&graph, hold(&[10]));
    let future = TreatmentCommand {
        occurred: Occurrence::Known(at(101)),
        ..original.clone()
    };
    assert_eq!(
        future.plan(&catalog(), &graph, &state, id(30), actor(), at(100)),
        Err(TreatmentError::InvalidOccurrence)
    );
    apply(&mut state, &mut graph, 30, hold(&[10]));
    assert_eq!(
        original.plan(&catalog(), &graph, &state, id(31), actor(), at(100)),
        Err(TreatmentError::StaleRevision)
    );
    state.holds[0].stamp.occurred = Occurrence::Known(at(90));
    let backwards = TreatmentCommand {
        occurred: Occurrence::Known(at(80)),
        ..command(&graph, release(30))
    };
    assert_eq!(
        backwards.plan(&catalog(), &graph, &state, id(31), actor(), at(100)),
        Err(TreatmentError::InvalidOccurrence)
    );
    assert!(state.holds[0].release.is_none());
    apply(&mut state, &mut graph, 31, release(30));
    let before = state.clone();
    assert!(
        command(&graph, release(30))
            .plan(&catalog(), &graph, &state, id(32), actor(), at(100))
            .is_err()
    );
    assert_eq!(state, before);
}

#[test]
fn damaged_foreign_or_duplicate_original_history_is_rejected() {
    let mut graph = graph(2, vec![node(10, None, 2, 20)]);
    let mut state = TreatmentState::default();
    apply(&mut state, &mut graph, 30, hold(&[10]));
    let mut duplicate = state.clone();
    duplicate.holds.push(duplicate.holds[0].clone());
    assert!(
        duplicate
            .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
            .is_err()
    );
    let foreign = PhysicalLineage::new(
        HubId::from_bytes([2; 16]).unwrap(),
        graph.trace(),
        graph.variant(),
        graph.revision(),
        graph.nodes().to_vec(),
    )
    .unwrap();
    assert!(
        state
            .project(&foreign, SampleHandlingBasis::WHOLE_PACK_V1)
            .is_err()
    );
    let mut damaged = state.clone();
    damaged.holds[0].stamp.stock_revision = rev(2);
    assert!(
        damaged
            .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
            .is_err()
    );
    let mut damaged = state;
    damaged.holds[0].stamp.occurred = Occurrence::Known(at(101));
    assert_eq!(
        damaged.project(&graph, SampleHandlingBasis::WHOLE_PACK_V1),
        Err(TreatmentError::InvalidOccurrence)
    );
}

#[test]
fn bounded_history_selection_and_summary_fail_closed_at_limits() {
    assert_eq!(
        TreatmentSelection::new(vec![]),
        Err(TreatmentError::InvalidSelection)
    );
    assert!(TreatmentSelection::new(vec![id(1); 2]).is_err());
    assert!(TreatmentSelection::new((1..=65).map(id).collect()).is_err());
    assert_eq!(
        TreatmentSelection::new((1..=64).map(id).collect())
            .unwrap()
            .groups()
            .len(),
        64
    );
    assert_eq!(
        TreatmentSelection::new(vec![id(2), id(1)]).unwrap(),
        TreatmentSelection::new(vec![id(1), id(2)]).unwrap()
    );
    let mut graph = graph(2, vec![node(10, None, 2, 20)]);
    let mut state = TreatmentState::default();
    for n in 0..HUB_TREATMENT_RECORDS_PER_TRACE {
        let mut bytes = [0_u8; 16];
        bytes[..8].copy_from_slice(&(n as u64 + 1).to_be_bytes());
        bytes[15] = 1;
        let plan = command(&graph, hold(&[10]))
            .plan(
                &catalog(),
                &graph,
                &state,
                HubRecordId::from_bytes(bytes).unwrap(),
                actor(),
                at(100),
            )
            .unwrap();
        let TreatmentChange::HoldPlaced(value) = plan.change else {
            panic!("hold")
        };
        state.holds.push(*value);
        graph = PhysicalLineage::new(
            graph.hub(),
            graph.trace(),
            graph.variant(),
            plan.revision,
            graph.nodes().to_vec(),
        )
        .unwrap();
    }
    let projected = state
        .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
        .unwrap();
    assert_eq!(projected.summary.restricted, packs(20));
    assert_eq!(projected.groups[0].active_holds.len(), 256);
    assert_eq!(
        command(&graph, hold(&[10])).plan(&catalog(), &graph, &state, id(99), actor(), at(100)),
        Err(TreatmentError::Capacity)
    );
    // Capacity stops new primary decisions, while an existing hold can still close.
    let closed = command(
        &graph,
        TreatmentAction::ReleaseHold {
            hold: state.holds()[0].stamp.id,
            expected_revision: HubRevision::INITIAL,
        },
    )
    .plan(&catalog(), &graph, &state, id(99), actor(), at(100))
    .unwrap();
    assert_eq!(closed.summary.restricted, packs(20));
    let impossible = TreatmentSummary {
        on_hand: packs(20),
        accepted: packs(0),
        review_required: packs(20),
        rejected: packs(0),
        restricted: packs(0),
        departure_eligible: packs(1),
    };
    assert_eq!(impossible.validate(), Err(TreatmentError::InvalidHistory));
    let impossible_unheld = TreatmentSummary {
        accepted: packs(20),
        review_required: packs(0),
        departure_eligible: packs(0),
        ..impossible
    };
    assert_eq!(
        impossible_unheld.validate(),
        Err(TreatmentError::InvalidHistory)
    );
    let exhausted = super::tests::graph(HubRevision::MAXIMUM, vec![node(10, None, 2, 20)]);
    assert_eq!(
        command(&exhausted, hold(&[10])).plan(
            &catalog(),
            &exhausted,
            &TreatmentState::default(),
            id(99),
            actor(),
            at(100)
        ),
        Err(TreatmentError::RevisionExhausted)
    );
}

#[test]
fn treatment_intent_and_record_identity_have_stable_distinct_vectors() {
    let graph = graph(2, vec![node(10, None, 2, 20)]);
    let hold = command(&graph, hold(&[10]));
    let accepted = command(&graph, accept(&[10], AcceptanceStatus::Accepted));
    let release = command(&graph, release(30));
    let hashes = [
        hold.fingerprint(graph.hub(), actor(), id(30)),
        accepted.fingerprint(graph.hub(), actor(), id(30)),
        release.fingerprint(graph.hub(), actor(), id(30)),
    ];
    assert!(hashes[0] != hashes[1] && hashes[1] != hashes[2] && hashes[0] != hashes[2]);
    assert_eq!(
        hashes,
        [
            [
                229, 71, 71, 166, 135, 56, 108, 168, 183, 166, 59, 66, 180, 12, 18, 71, 218, 76,
                12, 231, 196, 64, 218, 195, 19, 24, 124, 1, 138, 187, 196, 184
            ],
            [
                131, 159, 171, 193, 214, 171, 161, 104, 227, 114, 43, 14, 234, 16, 30, 153, 2, 153,
                114, 21, 251, 198, 108, 185, 137, 229, 188, 73, 219, 72, 78, 213
            ],
            [
                121, 204, 118, 247, 244, 159, 179, 149, 60, 122, 104, 93, 130, 171, 146, 109, 210,
                187, 160, 65, 42, 77, 95, 230, 111, 13, 215, 204, 24, 160, 174, 33
            ]
        ]
    );
    assert_eq!(
        treatment_record_id(id(20)).unwrap().as_bytes(),
        &[
            44, 149, 153, 0, 47, 123, 204, 93, 173, 176, 160, 173, 30, 2, 61, 57
        ]
    );
}

#[test]
fn treatment_fingerprint_binds_scope_versions_occurrence_and_original_context() {
    let graph = graph(2, vec![node(10, None, 2, 20)]);
    let original = command(&graph, hold(&[10]));
    let digest = original.fingerprint(graph.hub(), actor(), id(30));
    for changed in [
        TreatmentCommand {
            expected_stock_revision: rev(3),
            ..original.clone()
        },
        TreatmentCommand {
            expected_catalog_revision: rev(4),
            ..original.clone()
        },
        TreatmentCommand {
            action: hold(&[11]),
            ..original.clone()
        },
        TreatmentCommand {
            occurred: Occurrence::Known(at(90)),
            ..original.clone()
        },
        TreatmentCommand {
            observation: SourceProvenance::new("Different observed reason").unwrap(),
            ..original.clone()
        },
    ] {
        assert_ne!(digest, changed.fingerprint(graph.hub(), actor(), id(30)));
    }
    assert_ne!(
        digest,
        original.fingerprint(HubId::from_bytes([2; 16]).unwrap(), actor(), id(30))
    );
    assert_ne!(digest, original.fingerprint(graph.hub(), actor(), id(31)));
    let other =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    assert_ne!(digest, original.fingerprint(graph.hub(), other, id(30)));
    let release = command(&graph, release(30));
    let changed = TreatmentCommand {
        action: TreatmentAction::ReleaseHold {
            hold: id(30),
            expected_revision: rev(2),
        },
        ..release.clone()
    };
    assert_ne!(
        release.fingerprint(graph.hub(), actor(), id(31)),
        changed.fingerprint(graph.hub(), actor(), id(31))
    );
}

#[test]
fn full_primary_history_blocks_departure_when_a_new_precautionary_hold_cannot_fit() {
    let mut graph = graph(2, vec![node(10, None, 2, 20)]);
    let mut state = TreatmentState::default();
    for n in 0..HUB_TREATMENT_RECORDS_PER_TRACE {
        let mut bytes = [0_u8; 16];
        bytes[..8].copy_from_slice(&(n as u64 + 1).to_be_bytes());
        bytes[15] = 1;
        let plan = command(&graph, accept(&[10], AcceptanceStatus::Accepted))
            .plan(
                &catalog(),
                &graph,
                &state,
                HubRecordId::from_bytes(bytes).unwrap(),
                actor(),
                at(100),
            )
            .unwrap();
        let TreatmentChange::Acceptance(value) = plan.change else {
            panic!("acceptance")
        };
        state.decisions.push(*value);
        graph = PhysicalLineage::new(
            graph.hub(),
            graph.trace(),
            graph.variant(),
            plan.revision,
            graph.nodes().to_vec(),
        )
        .unwrap();
    }
    let projection = state
        .project(&graph, SampleHandlingBasis::WHOLE_PACK_V1)
        .unwrap();
    assert_eq!(projection.summary.departure_eligible, packs(20));
    assert!(projection.history_capacity_reached);
    assert_eq!(
        projection.require(&selection(&[10]), PhysicalAction::NormalDeparture),
        Err(TreatmentError::Capacity)
    );
    projection
        .require(&selection(&[10]), PhysicalAction::InternalRelocation)
        .unwrap();
    assert_eq!(
        command(&graph, hold(&[10])).plan(&catalog(), &graph, &state, id(99), actor(), at(100)),
        Err(TreatmentError::Capacity)
    );
}
