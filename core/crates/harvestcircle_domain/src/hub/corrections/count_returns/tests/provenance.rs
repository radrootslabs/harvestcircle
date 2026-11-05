use super::*;

// These are isolated graph-contract tests. Deliberately changed edges are not
// full Return/Count originals and establish no native completeness or allowance.
fn edge(from: HubRecordId, to: HubRecordId) -> CountReturnAssociation {
    let mut row = Fixture::new().plan().unwrap().association;
    row.returned_population = from;
    row.explanation_trace = to;
    row
}

#[test]
fn two_and_three_cohort_count_cycles_are_rejected_but_acyclic_chains_survive() {
    let a = id(1);
    let b = id(2);
    let c = id(3);
    let d = id(4);
    let first = edge(a, b);
    assert_eq!(
        audit_provenance(std::slice::from_ref(&first), &[], b, a),
        Err(CountReturnAssociationError::InvalidFacts)
    );
    assert!(audit_provenance(std::slice::from_ref(&first), &[], c, a).is_ok());
    let second = edge(b, c);
    assert_eq!(
        audit_provenance(&[first.clone(), second.clone()], &[], c, a),
        Err(CountReturnAssociationError::InvalidFacts)
    );
    assert!(audit_provenance(&[first, second], &[], d, a).is_ok());
}

#[test]
fn unrelated_existing_cycle_and_duplicate_outgoing_edges_are_invalid_history() {
    assert_eq!(
        audit_provenance(&[edge(id(1), id(2)), edge(id(2), id(1))], &[], id(3), id(4)),
        Err(CountReturnAssociationError::InvalidHistory)
    );
    assert_eq!(
        audit_provenance(&[edge(id(1), id(2)), edge(id(1), id(3))], &[], id(4), id(5)),
        Err(CountReturnAssociationError::InvalidHistory)
    );
    assert_eq!(
        audit_provenance(&[edge(id(1), id(2))], &[], id(1), id(3)),
        Err(CountReturnAssociationError::AlreadyAssociated)
    );
}

#[test]
fn complete_normal_original_supplies_an_edge_and_mixed_cycle_is_rejected() {
    let (hub, original, digest) = crate::hub::returns::normal_return_policy_fixture();
    let normal = NormalReturnProvenance::from_original(hub, &original, digest).unwrap();
    assert_eq!(normal.population, original.identities.population);
    assert_eq!(
        normal.source_trace,
        original
            .linkage
            .claimed_source
            .as_ref()
            .unwrap()
            .original
            .trace
            .id
    );
    assert_eq!(normal.original_digest, digest);
    assert_eq!(
        audit_provenance(
            &[edge(normal.population, id(243))],
            std::slice::from_ref(&normal),
            id(244),
            id(245)
        ),
        Err(CountReturnAssociationError::InvalidHistory)
    );
    assert_eq!(
        audit_provenance(
            &[],
            std::slice::from_ref(&normal),
            normal.source_trace,
            normal.population
        ),
        Err(CountReturnAssociationError::InvalidFacts)
    );
    let tail = edge(normal.source_trace, id(245));
    assert_eq!(
        audit_provenance(
            &[tail],
            std::slice::from_ref(&normal),
            id(245),
            normal.population
        ),
        Err(CountReturnAssociationError::InvalidFacts)
    );
    assert!(audit_provenance(&[], &[normal], id(244), original.identities.population).is_ok());
}

#[test]
fn unknown_unrecorded_and_contradictory_claims_supply_no_normal_edge() {
    let f = Fixture::new();
    assert_eq!(
        NormalReturnProvenance::from_original(f.original.known_event.hub, &f.returned, [0; 32]),
        Err(CountReturnAssociationError::InvalidHistory)
    );
    let (hub, original, digest) = crate::hub::returns::normal_return_policy_fixture();
    for fault in 0..5 {
        let mut changed = original.clone();
        match fault {
            0 => changed.linkage.resolution = ReturnLinkResolution::Unknown,
            1 => changed.linkage.claim = None,
            2 => changed.linkage.claim.as_mut().unwrap().group = Some(id(247)),
            3 => changed.linkage.timing = ReturnTiming::Contradictory,
            _ => changed.intake.population = id(248),
        }
        assert_eq!(
            NormalReturnProvenance::from_original(hub, &changed, digest),
            Err(CountReturnAssociationError::InvalidHistory)
        );
    }
}

#[test]
fn normal_original_timing_is_recomputed_and_occurrences_obey_original_clocks() {
    let (hub, original, digest) = crate::hub::returns::normal_return_policy_fixture();
    assert!(NormalReturnProvenance::from_original(hub, &original, digest).is_ok());
    let mut known = original;
    let departure_at = known.linkage.departure.as_ref().unwrap().recorded_at;
    known.linkage.departure.as_mut().unwrap().occurred = Occurrence::Known(departure_at);
    known.intake.occurred = Occurrence::Known(known.intake.recorded_at);
    known.linkage.timing = ReturnTiming::AfterOrSame;
    assert!(NormalReturnProvenance::from_original(hub, &known, digest).is_ok());
    let mut bad = known.clone();
    bad.intake.occurred = Occurrence::Known(at(departure_at.as_seconds() - 1));
    assert_eq!(
        NormalReturnProvenance::from_original(hub, &bad, digest),
        Err(CountReturnAssociationError::InvalidHistory)
    );
    bad = known.clone();
    bad.linkage.timing = ReturnTiming::Uncertain;
    assert_eq!(
        NormalReturnProvenance::from_original(hub, &bad, digest),
        Err(CountReturnAssociationError::InvalidHistory)
    );
    bad = known.clone();
    bad.intake.occurred = Occurrence::Known(at(bad.intake.recorded_at.as_seconds() + 1));
    assert_eq!(
        NormalReturnProvenance::from_original(hub, &bad, digest),
        Err(CountReturnAssociationError::InvalidHistory)
    );
    bad = known;
    bad.linkage.departure.as_mut().unwrap().occurred =
        Occurrence::Known(at(departure_at.as_seconds() + 1));
    assert_eq!(
        NormalReturnProvenance::from_original(hub, &bad, digest),
        Err(CountReturnAssociationError::InvalidHistory)
    );
}

#[test]
fn graph_walk_is_iterative_bounded_and_retains_depleted_original_edges() {
    fn node(n: usize) -> HubRecordId {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&(n as u64 + 1).to_be_bytes());
        HubRecordId::from_bytes(bytes).unwrap()
    }
    let template = Fixture::new().plan().unwrap().association;
    let rows = (0..HUB_TRACE_CAPACITY - 2)
        .map(|n| {
            let mut row = template.clone();
            row.returned_population = node(n);
            row.explanation_trace = node(n + 1);
            row
        })
        .collect::<Vec<_>>();
    assert!(audit_provenance(&rows, &[], node(HUB_TRACE_CAPACITY - 1), node(0)).is_ok());
    assert_eq!(
        audit_provenance(&rows, &[], node(HUB_TRACE_CAPACITY - 2), node(0)),
        Err(CountReturnAssociationError::InvalidFacts)
    );
    let mut full = rows;
    let mut last = template;
    last.returned_population = node(HUB_TRACE_CAPACITY - 2);
    last.explanation_trace = node(HUB_TRACE_CAPACITY - 1);
    full.push(last);
    assert_eq!(
        audit_provenance(&full, &[], node(HUB_TRACE_CAPACITY), node(0)),
        Err(CountReturnAssociationError::Capacity)
    );
}
