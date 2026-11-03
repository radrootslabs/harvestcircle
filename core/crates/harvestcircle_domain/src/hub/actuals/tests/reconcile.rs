use super::source::witness;
use super::*;

#[test]
fn historical_actual_preserves_later_native_floor_and_fences_new_handling() {
    let mut f = Fixture::new(known_occurrence(120));
    f.group.causal_floor = Some(at(130));
    let reduction = ActualPhysicalReduction::plan(&f.group, packs(2)).unwrap();
    assert!(
        f.group
            .observation_floor(known_occurrence(120), at(170))
            .is_err()
    );
    for occurred in [
        known_occurrence(120),
        interval(120, 140),
        Occurrence::Unknown,
    ] {
        assert_eq!(
            reduction
                .causal_floor_after_reconciliation(occurred, at(170))
                .unwrap(),
            Some(at(130))
        );
    }
    assert_eq!(
        reduction
            .causal_floor_after_reconciliation(known_occurrence(140), at(170))
            .unwrap(),
        Some(at(140))
    );
    assert_eq!(
        reduction.causal_floor_after_reconciliation(known_occurrence(171), at(170)),
        Err(ActualReportError::InvalidInput)
    );
    assert_eq!(
        reduction.causal_floor_after_reconciliation(Occurrence::Unknown, at(120)),
        Err(ActualReportError::InvalidNativeContext)
    );
    f.group.causal_floor = None;
    let reduction = ActualPhysicalReduction::plan(&f.group, packs(2)).unwrap();
    assert_eq!(
        reduction
            .causal_floor_after_reconciliation(Occurrence::Unknown, at(170))
            .unwrap(),
        None
    );
    assert_eq!(
        reduction
            .causal_floor_after_reconciliation(interval(120, 140), at(170))
            .unwrap(),
        Some(at(120))
    );
}

fn command(f: &Fixture) -> ActualNewDepartureCommand {
    ActualNewDepartureCommand {
        facts: f.facts.clone(),
        source_operation: id(200),
        source_group: f.group.id,
        duplicate_review_basis: note(
            "Reviewed every original departure; this separately observed event has no recorded original",
        ),
    }
}
fn plan(
    f: &Fixture,
    source: &HistoricalActualSource,
    known: &[ActualKnownDeparture],
    prior: Option<HubRecordId>,
) -> Result<ActualNewDeparturePlan, ActualReportError> {
    command(f).plan(
        &ActualNewDepartureContext {
            review: f.context(),
            historical_source: ActualSourceContext {
                source,
                lineage: &f.graph,
                balancing_group: &f.group,
                subsequent_activity: &[],
            },
            known_departures: known,
            prior_review: prior,
        },
        id(230),
        owner(),
        at(170),
    )
}
fn advance(f: &mut Fixture) {
    let revision = HubRevision::from_value(f.graph.revision().value() + 2).unwrap();
    f.graph = PhysicalLineage::new(
        f.graph.hub(),
        f.graph.trace(),
        f.graph.variant(),
        revision,
        f.graph.nodes().to_vec(),
    )
    .unwrap();
    f.facts.expected_stock_revision = revision;
}
fn known(f: &Fixture) -> ActualKnownDeparture {
    ActualKnownDeparture {
        actor: owner(),
        operation: id(231),
        hub: f.graph.hub(),
        stock_revision: HubRevision::from_value(f.original.stock_revision.value() + 1).unwrap(),
        catalog_revision: f.catalog.revision(),
        trace: f.group.trace,
        variant: f.catalog.get(f.group.variant).unwrap().clone(),
        recipient: f.catalog.get(f.facts.recipient).unwrap().clone(),
        occurred: f.facts.occurred,
        recorded_at: at(160),
        lines: vec![ActualKnownDepartureLine {
            group: f.group.id,
            location: f.catalog.get(f.group.location).unwrap().clone(),
            packs: packs(2),
        }],
    }
}

#[test]
fn actual_reduction_retires_only_two_pending_and_preserves_original_twenty_required() {
    let f = Fixture::new(known_time());
    let source = witness(&f);
    let plan = plan(&f, &source, &[], None).unwrap();
    assert_eq!(plan.reduction.actual_departed, packs(2));
    assert_eq!(plan.reduction.on_hand_after, packs(4));
    let change = &plan.reduction.work_changes[0];
    assert_eq!(change.after.quantity.required(), packs(20));
    assert_eq!(change.after.quantity.completed(), packs(0));
    assert_eq!(change.after.quantity.no_longer_required(), packs(2));
    assert_eq!(change.after.quantity.remaining(), packs(18));
    assert_eq!(
        plan.reduction.removed_work_scope[0].removed_progress,
        WorkProgress::NoLongerRequired
    );
    assert_eq!(plan.report, f.original);
    assert_eq!(plan.historical_source.historical(), &source);
    assert_eq!(f.group.packs, packs(6));
    plan.reduction.validate().unwrap();
}
fn known_time() -> Occurrence {
    known_occurrence(120)
}
fn known_occurrence(n: i64) -> Occurrence {
    Occurrence::Known(at(n))
}

#[test]
fn completed_and_already_retired_history_are_not_reclassified_as_pending_departure_work() {
    for progress in [WorkProgress::Completed, WorkProgress::NoLongerRequired] {
        let mut f = Fixture::new(known_time());
        f.group.work[0].progress = progress;
        f.group.work[0].requirement.quantity = crate::hub::stock::QuantityWork::restore(
            packs(20),
            packs(if progress == WorkProgress::Completed {
                20
            } else {
                0
            }),
            packs(if progress == WorkProgress::NoLongerRequired {
                20
            } else {
                0
            }),
            packs(0),
        )
        .unwrap();
        let reduction = ActualPhysicalReduction::plan(&f.group, packs(2)).unwrap();
        assert!(reduction.work_changes.is_empty());
        assert_eq!(reduction.removed_work_scope[0].removed_progress, progress);
        assert_eq!(reduction.before.work, f.group.work);
    }
}

#[test]
fn full_exit_keeps_empty_historical_scope_and_partial_or_corrupt_effect_cannot_invent_balance() {
    let f = Fixture::new(known_time());
    let full = ActualPhysicalReduction::plan(&f.group, packs(6)).unwrap();
    assert_eq!(full.on_hand_after, packs(0));
    full.validate().unwrap();
    for packs in [packs(0), packs(7)] {
        assert_eq!(
            ActualPhysicalReduction::plan(&f.group, packs),
            Err(ActualReportError::InsufficientBalance)
        );
    }
    let mut broken = full.clone();
    broken.on_hand_after = packs(1);
    assert_eq!(broken.validate(), Err(ActualReportError::InvalidHistory));
    let mut broken = full;
    broken.work_changes[0].after.quantity = crate::hub::stock::QuantityWork::pending(packs(20));
    assert_eq!(broken.validate(), Err(ActualReportError::InvalidHistory));
}

#[test]
fn held_actual_event_is_factual_and_keeps_independent_hold_history_unchanged() {
    let mut f = Fixture::new(known_time());
    let source = witness(&f);
    f.hold(known_occurrence(110), None);
    let hold = f.state.holds()[0].clone();
    let plan = plan(&f, &source, &[], None).unwrap();
    assert_eq!(
        plan.comparison.potential_historical_holds[0].conditional_timing,
        HistoricalHoldTiming::EstablishedBreach
    );
    assert_eq!(plan.comparison.potential_historical_holds[0].original, hold);
    assert_eq!(f.state.holds()[0], hold);
    assert!(f.state.holds()[0].release.is_none());
    assert_eq!(
        plan.comparison.potential_historical_holds[0].applicability,
        HistoricalHoldApplicability::HistoricalScopeUnverified
    );
}

#[test]
fn arrival_snapshot_and_recorded_lineage_do_not_establish_physical_hold_anchor_birth() {
    let mut f = Fixture::new(known_time());
    let mut source = witness(&f);
    source.occurred = known_occurrence(115);
    source.recorded_at = at(115);
    f.hold(known_occurrence(110), None);
    let plan = plan(&f, &source, &[], None).unwrap();
    let finding = &plan.comparison.potential_historical_holds[0];
    assert_eq!(
        finding.conditional_timing,
        HistoricalHoldTiming::EstablishedBreach
    );
    assert_eq!(
        finding.applicability,
        HistoricalHoldApplicability::HistoricalScopeUnverified
    );
    assert_eq!(finding.original, f.state.holds()[0]);
    assert_eq!(plan.historical_source.historical(), &source);
}

#[test]
fn a_possible_duplicate_cannot_be_cleared_by_a_new_identity_or_a_verification_reason() {
    let mut f = Fixture::new(known_time());
    advance(&mut f);
    let source = witness(&f);
    let old = known(&f);
    assert_eq!(
        plan(&f, &source, std::slice::from_ref(&old), None),
        Err(ActualReportError::DuplicateReviewRequired)
    );
    let mut smaller = old;
    smaller.lines[0].packs = packs(1);
    assert_eq!(
        plan(&f, &source, &[smaller], None),
        Err(ActualReportError::DuplicateReviewRequired)
    );
}

#[test]
fn disjoint_known_occurrence_is_not_a_duplicate_but_unknown_previous_time_stays_for_review() {
    let mut f = Fixture::new(known_time());
    advance(&mut f);
    let source = witness(&f);
    let mut old = known(&f);
    old.occurred = known_occurrence(110);
    assert!(plan(&f, &source, &[old.clone()], None).is_ok());
    old.occurred = Occurrence::Unknown;
    assert_eq!(
        plan(&f, &source, &[old], None),
        Err(ActualReportError::DuplicateReviewRequired)
    );
}

#[test]
fn native_review_and_source_bindings_are_mandatory_even_for_consistent_physical_math() {
    let f = Fixture::new(known_time());
    let source = witness(&f);
    assert_eq!(
        plan(&f, &source, &[], Some(id(240))),
        Err(ActualReportError::AlreadyReviewed)
    );
    let mut source = source;
    source.operation = id(241);
    assert_eq!(
        plan(&f, &source, &[], None),
        Err(ActualReportError::InvalidNativeContext)
    );
    let source = witness(&f);
    let mut changed = command(&f);
    changed.source_group = id(246);
    assert_eq!(
        changed.plan(
            &ActualNewDepartureContext {
                review: f.context(),
                historical_source: ActualSourceContext {
                    source: &source,
                    lineage: &f.graph,
                    balancing_group: &f.group,
                    subsequent_activity: &[]
                },
                known_departures: &[],
                prior_review: None
            },
            id(230),
            owner(),
            at(170)
        ),
        Err(ActualReportError::InvalidNativeContext)
    );
}

#[test]
fn duplicate_link_preserves_original_event_without_needing_any_current_on_hand_group() {
    let mut f = Fixture::new(known_time());
    advance(&mut f);
    let old = known(&f);
    let link = ActualDuplicateLink {
        report: f.original.id,
        known_operation: old.operation,
        known_trace: old.trace,
        known_variant: old.variant.id,
        verification: note(
            "Witness confirmed this is the same already recorded native departure, no second physical effect",
        ),
    };
    let result = link
        .plan(&f.original, &old, None, id(242), owner(), at(170))
        .unwrap();
    assert_eq!(result.original, f.original);
    assert_eq!(result.known, old);
    assert_eq!(result.known.packs().unwrap(), packs(2));
    assert_eq!(
        link.plan(&f.original, &old, Some(id(243)), id(242), owner(), at(170)),
        Err(ActualReportError::AlreadyReviewed)
    );
    let mut wrong = old.clone();
    wrong.lines[0].packs = packs(3);
    assert_eq!(
        link.plan(&f.original, &wrong, None, id(242), owner(), at(170)),
        Err(ActualReportError::ConflictingEvidence)
    );
    let mut wrong_link = link;
    wrong_link.known_trace = id(246);
    assert_eq!(
        wrong_link.plan(&f.original, &old, None, id(242), owner(), at(170)),
        Err(ActualReportError::InvalidNativeContext)
    );
}

#[test]
fn duplicate_original_lines_are_bounded_distinct_and_checked_before_linking() {
    let mut f = Fixture::new(known_time());
    advance(&mut f);
    let mut old = known(&f);
    old.lines.push(old.lines[0].clone());
    assert_eq!(old.validate(), Err(ActualReportError::InvalidHistory));
    old.lines.clear();
    assert_eq!(old.validate(), Err(ActualReportError::Capacity));
}

#[test]
fn distinct_verification_source_time_and_operation_intents_have_distinct_fingerprints() {
    let f = Fixture::new(known_time());
    let original = command(&f);
    let digest = original.fingerprint(f.graph.hub(), owner(), id(230));
    let mut changes = Vec::new();
    let mut changed = original.clone();
    changed.source_operation = id(249);
    changes.push(changed);
    let mut changed = original.clone();
    changed.source_group = id(246);
    changes.push(changed);
    let mut changed = original.clone();
    changed.facts.occurred = interval(119, 121);
    changes.push(changed);
    let mut changed = original.clone();
    changed.facts.source_evidence = note("Different verified physical source evidence");
    changes.push(changed);
    let mut changed = original.clone();
    changed.facts.packs = packs(3);
    changes.push(changed);
    let mut changed = original.clone();
    changed.duplicate_review_basis = note("Different original duplicate investigation basis");
    changes.push(changed);
    for changed in changes {
        assert_ne!(changed.fingerprint(f.graph.hub(), owner(), id(230)), digest);
    }
    assert_ne!(
        original.fingerprint(f.graph.hub(), owner(), id(248)),
        digest
    );
    let original = ActualDuplicateLink {
        report: f.original.id,
        known_operation: id(231),
        known_trace: f.group.trace,
        known_variant: f.group.variant,
        verification: note("Confirmed the immutable original event"),
    };
    let mut changed = original.clone();
    changed.known_operation = id(247);
    assert_ne!(
        original.fingerprint(f.graph.hub(), owner(), id(230)),
        changed.fingerprint(f.graph.hub(), owner(), id(230))
    );
}
