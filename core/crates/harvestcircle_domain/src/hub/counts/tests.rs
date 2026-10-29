use super::*;
use crate::hub::movement::{WorkAllocation, WorkProgress, WorkRequirement};
use crate::hub::reference::{HubLabel, LocationPurpose};
use crate::hub::stock::QuantityWork;

#[path = "result_tests.rs"]
mod result_tests;

#[path = "scope_tests.rs"]
mod scope_tests;

#[path = "surplus_tests.rs"]
mod surplus_tests;

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn text(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
fn actor() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn hub() -> HubId {
    HubId::from_bytes([1; 16]).unwrap()
}
fn fixture() -> (ReferenceCatalog, PhysicalStockGroup, CountObserveCommand) {
    let mut catalog = ReferenceCatalog::default();
    for (n, value) in [
        (
            1,
            ReferenceValue::Product {
                label: HubLabel::new("Fictional apples").unwrap(),
            },
        ),
        (
            2,
            ReferenceValue::Pack {
                product: id(1),
                label: HubLabel::new("Original crate").unwrap(),
                nominal_grams: None,
            },
        ),
        (
            3,
            ReferenceValue::Location {
                label: HubLabel::new("Receiving").unwrap(),
                purpose: LocationPurpose::Receiving,
            },
        ),
        (
            4,
            ReferenceValue::Location {
                label: HubLabel::new("A3").unwrap(),
                purpose: LocationPurpose::Storage,
            },
        ),
    ] {
        catalog.create(catalog.revision(), id(n), value).unwrap();
    }
    let group = PhysicalStockGroup {
        id: id(10),
        trace: id(11),
        variant: id(2),
        parent: None,
        identification: None,
        location: id(3),
        packs: packs(20),
        revision: rev(1),
        causal_floor: Some(at(40)),
        work: vec![WorkAllocation {
            requirement: WorkRequirement {
                id: id(12),
                trace: id(11),
                variant: id(2),
                target: Some(
                    catalog
                        .require_kind(id(4), ReferenceKind::Location)
                        .unwrap()
                        .clone(),
                ),
                quantity: QuantityWork::pending(packs(20)),
                revision: rev(1),
            },
            progress: WorkProgress::Pending,
        }],
    };
    let command = CountObserveCommand {
        expected_stock_revision: rev(2),
        expected_catalog_revision: catalog.revision(),
        group: group.id,
        expected_group_revision: group.revision,
        counted_packs: packs(18),
        cutoff: Occurrence::Known(at(80)),
        observation: text("Counted eighteen original crates within the identified group"),
    };
    (catalog, group, command)
}
fn context<'a>(catalog: &'a ReferenceCatalog, group: &'a PhysicalStockGroup) -> CountContext<'a> {
    CountContext {
        hub: hub(),
        stock_revision: rev(2),
        catalog,
        group,
        latest_physical_recorded_at: at(50),
        previous_reconciliation: None,
        latest_observation: None,
        unresolved_actual: false,
        uncertain_intent: false,
    }
}
fn observation(
    catalog: &ReferenceCatalog,
    group: &PhysicalStockGroup,
    command: &CountObserveCommand,
) -> CountObservation {
    command
        .plan(
            &context(catalog, group).observation_context(),
            id(20),
            actor(),
            at(100),
        )
        .unwrap()
}
fn post(count: &CountObservation, current: &CountContext<'_>) -> CountPostCommand {
    CountPostCommand {
        expected_stock_revision: current.stock_revision,
        expected_catalog_revision: current.catalog.revision(),
        count: count.id,
        reviewed_activity: true,
        surplus_identification: None,
        reason: text(
            "Reviewed scope and activity; eighteen crates remain and the missing two are unexplained",
        ),
    }
}

#[test]
fn observing_eighteen_leaves_recorded_twenty_and_work_unchanged() {
    let (catalog, group, command) = fixture();
    let before = group.clone();
    let count = observation(&catalog, &group, &command);
    assert_eq!(count.scope.recorded_packs, packs(20));
    assert_eq!(count.counted_packs, packs(18));
    assert_eq!(count.recorded_revision, rev(3));
    assert_eq!(group, before);
    assert_eq!(
        group.work[0].requirement.quantity,
        QuantityWork::pending(packs(20))
    );
    assert_eq!(
        count.difference().unwrap(),
        CountDifference::Shortage(packs(2))
    );
}

#[test]
fn reviewed_shortage_retires_only_two_pending_packs_without_rewriting_the_observation() {
    let (catalog, group, command) = fixture();
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let plan = post(&count, &current)
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    let CountEffect::Shortage {
        packs: missing,
        after_packs,
        after_revision,
        work_changes,
    } = &plan.effect
    else {
        panic!("shortage")
    };
    assert_eq!(
        (*missing, *after_packs, *after_revision),
        (packs(2), packs(18), rev(2))
    );
    let after = work_changes[0].after.quantity;
    assert_eq!(
        (
            after.required(),
            after.completed(),
            after.no_longer_required(),
            after.remaining()
        ),
        (packs(20), packs(0), packs(2), packs(18))
    );
    assert_eq!(
        work_changes[0].before.quantity,
        QuantityWork::pending(packs(20))
    );
    assert_eq!(plan.count, count);
    assert_eq!(plan.before, group);
    assert_eq!(plan.confirmed_on_hand_at, at(80));
    assert_eq!(plan.stock_revision, rev(4));
}

#[test]
fn zero_observation_can_retire_every_pending_pack_without_minting_a_zero_group() {
    let (catalog, group, mut command) = fixture();
    command.counted_packs = packs(0);
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let result = post(&count, &current)
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    let CountEffect::Shortage {
        after_packs,
        work_changes,
        ..
    } = result.effect
    else {
        panic!("shortage")
    };
    assert_eq!(after_packs, packs(0));
    assert_eq!(work_changes[0].after.quantity.remaining(), packs(0));
    assert_eq!(
        work_changes[0].after.quantity.no_longer_required(),
        packs(20)
    );
    assert_eq!(result.before.packs, packs(20));
}

#[test]
fn completed_work_is_historical_completion_and_is_never_undone_by_a_shortage() {
    let (catalog, mut group, command) = fixture();
    group.work[0].progress = WorkProgress::Completed;
    group.work[0].requirement.quantity =
        QuantityWork::restore(packs(20), packs(20), packs(0), packs(0)).unwrap();
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let plan = post(&count, &current)
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    let CountEffect::Shortage { work_changes, .. } = plan.effect else {
        panic!("shortage")
    };
    assert!(work_changes.is_empty());
    assert_eq!(
        plan.before.work[0].requirement.quantity.completed(),
        packs(20)
    );
}

#[test]
fn a_positive_difference_requires_separately_identified_new_stock_without_enlarging_original_scope()
{
    let (catalog, group, mut command) = fixture();
    command.counted_packs = packs(22);
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let mut posting = post(&count, &current);
    assert_eq!(
        posting.plan(&count, &current, id(21), actor(), at(110)),
        Err(CountError::InvalidInput)
    );
    let identification = PhysicalIdentification {
        label: HubLabel::new("Count surplus two").unwrap(),
        observation: text("Two extras separately tagged; original source unknown"),
    };
    posting.surplus_identification = Some(identification.clone());
    let result = posting
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    assert_eq!(
        result.effect,
        CountEffect::Surplus {
            packs: packs(2),
            identification,
            source: crate::hub::reference::SourceAttribution::Unknown,
            acceptance: crate::hub::treatment::AcceptanceStatus::ReviewRequired,
        }
    );
    assert_eq!(result.before, group);
    assert_eq!(result.count.scope.recorded_packs, packs(20));
}

#[test]
fn matching_count_is_reviewed_without_any_physical_or_work_effect() {
    let (catalog, group, mut command) = fixture();
    command.counted_packs = packs(20);
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let result = post(&count, &current)
        .plan(&count, &current, id(21), actor(), at(110))
        .unwrap();
    assert_eq!(result.effect, CountEffect::NoVariance);
    assert_eq!(result.before, group);
}

#[test]
fn unknown_and_interval_cutoffs_are_retained_observations_and_never_safe_posting_context() {
    let (catalog, group, mut command) = fixture();
    for cutoff in [
        Occurrence::Unknown,
        Occurrence::Interval {
            earliest: at(70),
            latest: at(80),
        },
    ] {
        command.cutoff = cutoff;
        let count = observation(&catalog, &group, &command);
        let mut current = context(&catalog, &group);
        current.stock_revision = rev(3);
        current.latest_observation = Some(count.id);
        assert_eq!(count.cutoff, cutoff);
        assert_eq!(
            count.review(&current).unwrap().blockers,
            vec![CountBlockReason::CutoffUncertain]
        );
        assert_eq!(
            post(&count, &current).plan(&count, &current, id(21), actor(), at(110)),
            Err(CountError::ReviewRequired)
        );
    }
}

#[test]
fn a_cutoff_before_confirmed_physical_activity_is_not_a_valid_comparison() {
    let (catalog, group, mut command) = fixture();
    command.cutoff = Occurrence::Known(at(45));
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    assert_eq!(
        count.review(&current).unwrap().blockers,
        vec![CountBlockReason::CutoffPrecedesRecordedActivity]
    );
}

#[test]
fn movement_newer_reconciliation_and_newer_count_cannot_be_hidden_by_equal_quantity() {
    let (catalog, group, command) = fixture();
    let count = observation(&catalog, &group, &command);
    let mut moved = group.clone();
    moved.revision = rev(2);
    moved.location = id(4);
    moved.work.clear();
    let mut current = context(&catalog, &moved);
    current.stock_revision = rev(6);
    current.previous_reconciliation = Some(id(30));
    current.latest_observation = Some(id(31));
    assert_eq!(
        count.review(&current).unwrap().blockers,
        vec![
            CountBlockReason::ScopeChanged,
            CountBlockReason::NewerReconciliation,
            CountBlockReason::NotLatestObservation
        ]
    );
    assert_eq!(
        post(&count, &current).plan(&count, &current, id(21), actor(), at(110)),
        Err(CountError::ReviewRequired)
    );
}

#[test]
fn unresolved_actual_facts_and_uncertain_prior_intents_block_post_even_with_matching_revisions() {
    let (catalog, group, command) = fixture();
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    current.unresolved_actual = true;
    current.uncertain_intent = true;
    assert_eq!(
        count.review(&current).unwrap().blockers,
        vec![
            CountBlockReason::UnresolvedActual,
            CountBlockReason::UncertainPriorOutcome
        ]
    );
    assert_eq!(
        post(&count, &current).plan(&count, &current, id(21), actor(), at(110)),
        Err(CountError::ReviewRequired)
    );
}

#[test]
fn unrelated_journal_changes_do_not_invent_a_physical_scope_change() {
    let (catalog, group, command) = fixture();
    let count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(10);
    current.latest_observation = Some(count.id);
    assert!(count.review(&current).unwrap().ready());
    assert_eq!(
        post(&count, &current)
            .plan(&count, &current, id(21), actor(), at(110))
            .unwrap()
            .stock_revision,
        rev(11)
    );
}

#[test]
fn stale_forms_unreviewed_activity_bad_clocks_and_malformed_originals_are_rejected() {
    let (catalog, group, mut command) = fixture();
    command.expected_stock_revision = rev(1);
    assert_eq!(
        command.plan(
            &context(&catalog, &group).observation_context(),
            id(20),
            actor(),
            at(100)
        ),
        Err(CountError::StaleRevision)
    );
    command.expected_stock_revision = rev(2);
    command.cutoff = Occurrence::Known(at(101));
    assert!(
        command
            .plan(
                &context(&catalog, &group).observation_context(),
                id(20),
                actor(),
                at(100)
            )
            .is_err()
    );
    command.cutoff = Occurrence::Known(at(80));
    let mut count = observation(&catalog, &group, &command);
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let mut posting = post(&count, &current);
    posting.reviewed_activity = false;
    assert_eq!(
        posting.plan(&count, &current, id(21), actor(), at(110)),
        Err(CountError::ReviewRequired)
    );
    posting.reviewed_activity = true;
    assert_eq!(
        posting.plan(&count, &current, id(21), actor(), at(99)),
        Err(CountError::InvalidHistory)
    );
    current.stock_revision = rev(2);
    assert_eq!(count.review(&current), Err(CountError::InvalidHistory));
    count.scope.product.id = id(99);
    assert_eq!(count.validate(), Err(CountError::InvalidHistory));
}

#[test]
fn identity_and_fingerprints_bind_original_count_review_and_optional_surplus_evidence() {
    let (catalog, group, command) = fixture();
    let count = observation(&catalog, &group, &command);
    assert_ne!(
        count_record_id(id(20)).unwrap(),
        count_record_id(id(21)).unwrap()
    );
    let fingerprint = command.fingerprint(hub(), actor(), id(20));
    let mut changed = command.clone();
    changed.counted_packs = packs(19);
    assert_ne!(fingerprint, changed.fingerprint(hub(), actor(), id(20)));
    changed = command.clone();
    changed.cutoff = Occurrence::Unknown;
    assert_ne!(fingerprint, changed.fingerprint(hub(), actor(), id(20)));
    assert_ne!(
        fingerprint,
        command.fingerprint(HubId::from_bytes([2; 16]).unwrap(), actor(), id(20))
    );
    let mut current = context(&catalog, &group);
    current.stock_revision = rev(3);
    current.latest_observation = Some(count.id);
    let mut posting = post(&count, &current);
    let fingerprint = posting.fingerprint(hub(), actor(), id(21));
    posting.reviewed_activity = false;
    assert_ne!(fingerprint, posting.fingerprint(hub(), actor(), id(21)));
    posting.reviewed_activity = true;
    posting.reason = text("Different reviewed reason");
    assert_ne!(fingerprint, posting.fingerprint(hub(), actor(), id(21)));
    posting = post(&count, &current);
    posting.surplus_identification = Some(PhysicalIdentification {
        label: HubLabel::new("Extra tagged crates").unwrap(),
        observation: text("Separately identified excess"),
    });
    assert_ne!(fingerprint, posting.fingerprint(hub(), actor(), id(21)));
}
