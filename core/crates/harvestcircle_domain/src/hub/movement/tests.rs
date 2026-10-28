use super::*;
use crate::hub::reference::{LocationPurpose, ReferenceValue};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn label(s: &str) -> HubLabel {
    HubLabel::new(s).unwrap()
}
fn basis(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn revision(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn catalog() -> ReferenceCatalog {
    let mut catalog = ReferenceCatalog::default();
    for (n, value) in [
        (
            1,
            ReferenceValue::Product {
                label: label("Fictional apples"),
            },
        ),
        (
            2,
            ReferenceValue::Pack {
                product: id(1),
                label: label("Original crate"),
                nominal_grams: None,
            },
        ),
        (
            3,
            ReferenceValue::Location {
                label: label("Receiving"),
                purpose: LocationPurpose::Receiving,
            },
        ),
        (
            4,
            ReferenceValue::Location {
                label: label("A3"),
                purpose: LocationPurpose::Storage,
            },
        ),
        (
            5,
            ReferenceValue::Location {
                label: label("B3"),
                purpose: LocationPurpose::Storage,
            },
        ),
    ] {
        catalog.create(catalog.revision(), id(n), value).unwrap();
    }
    catalog
}
fn group(catalog: &ReferenceCatalog) -> PhysicalStockGroup {
    PhysicalStockGroup {
        id: id(10),
        trace: id(11),
        variant: id(2),
        parent: None,
        identification: None,
        location: id(3),
        packs: packs(20),
        revision: HubRevision::INITIAL,
        causal_floor: None,
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
                revision: HubRevision::INITIAL,
            },
            progress: WorkProgress::Pending,
        }],
    }
}
fn command(
    catalog: &ReferenceCatalog,
    group: &PhysicalStockGroup,
    stock: u64,
    to: u8,
    count: u32,
) -> MovementCommand {
    MovementCommand {
        expected_stock_revision: revision(stock),
        expected_catalog_revision: catalog.revision(),
        group: group.id,
        expected_group_revision: group.revision,
        to: id(to),
        packs: packs(count),
        separated_portion: (count < group.packs.value()).then(|| PhysicalIdentification {
            label: label("Blue stack"),
            observation: basis("Eight original crates kept in the tagged blue stack"),
        }),
        occurred: Occurrence::Unknown,
        observation: basis("Manually moved the selected original crates"),
    }
}

#[test]
fn moving_eight_conserves_twenty_and_credits_only_actual_putaway() {
    let catalog = catalog();
    let original = group(&catalog);
    let plan = command(&catalog, &original, 1, 4, 8)
        .plan(&catalog, revision(1), &original, id(30), at(100))
        .unwrap();
    let remaining = plan.remaining.as_ref().unwrap();
    assert_eq!(
        (
            remaining.location,
            remaining.packs,
            plan.moved.location,
            plan.moved.packs
        ),
        (id(3), packs(12), id(4), packs(8))
    );
    assert_eq!(
        remaining.packs.checked_add(plan.moved.packs).unwrap(),
        original.packs
    );
    assert_eq!(plan.moved.parent, Some(original.id));
    assert_eq!(plan.moved.trace, original.trace);
    assert_eq!(plan.moved.variant, original.variant);
    let work = &plan.work_updates[0].quantity;
    assert_eq!(
        (
            work.required(),
            work.completed(),
            work.no_longer_required(),
            work.remaining()
        ),
        (packs(20), packs(8), packs(0), packs(12))
    );
    assert_eq!(remaining.work[0].progress, WorkProgress::Pending);
    assert_eq!(plan.moved.work[0].progress, WorkProgress::Completed);
    assert_eq!(original, group(&catalog));
}

#[test]
fn backward_repeated_and_partial_return_moves_cannot_double_credit() {
    let catalog = catalog();
    let original = group(&catalog);
    let first = command(&catalog, &original, 1, 4, 8)
        .plan(&catalog, revision(1), &original, id(30), at(100))
        .unwrap();
    let returned = command(&catalog, &first.moved, 2, 3, 8)
        .plan(&catalog, revision(2), &first.moved, id(31), at(100))
        .unwrap();
    assert!(returned.work_updates.is_empty());
    let repeated = command(&catalog, &returned.moved, 3, 4, 8)
        .plan(&catalog, revision(3), &returned.moved, id(32), at(100))
        .unwrap();
    assert!(repeated.work_updates.is_empty());
    assert_eq!(
        repeated.moved.work[0].requirement.quantity.completed(),
        packs(8)
    );
    let mut partial = command(&catalog, &repeated.moved, 4, 3, 4);
    partial.separated_portion.as_mut().unwrap().label = label("Green four");
    let split = partial
        .plan(&catalog, revision(4), &repeated.moved, id(33), at(100))
        .unwrap();
    assert!(split.work_updates.is_empty());
    assert_eq!(split.moved.work[0].progress, WorkProgress::Completed);
    assert_eq!(
        split.moved.work[0].requirement.quantity.completed(),
        packs(8)
    );
    let remaining = first.remaining.unwrap();
    let finish = command(&catalog, &remaining, 5, 4, 12)
        .plan(&catalog, revision(5), &remaining, id(34), at(100))
        .unwrap();
    assert_eq!(finish.work_updates[0].quantity.completed(), packs(20));
    assert_eq!(finish.work_updates[0].quantity.remaining(), packs(0));
}

#[test]
fn unidentified_partial_moves_and_silent_identity_rewrites_are_rejected() {
    let catalog = catalog();
    let original = group(&catalog);
    let mut partial = command(&catalog, &original, 1, 4, 8);
    partial.separated_portion = None;
    assert_eq!(
        partial.plan(&catalog, revision(1), &original, id(30), at(100)),
        Err(MovementError::PhysicalIdentificationRequired)
    );
    partial.separated_portion = Some(PhysicalIdentification {
        label: label("Blue"),
        observation: basis("Actual tagged crates"),
    });
    assert_eq!(
        partial.plan(&catalog, revision(1), &original, original.id, at(100)),
        Err(MovementError::IdentityConflict)
    );
    let mut whole = command(&catalog, &original, 1, 4, 20);
    whole.separated_portion = partial.separated_portion;
    assert_eq!(
        whole.plan(&catalog, revision(1), &original, id(30), at(100)),
        Err(MovementError::IdentityConflict)
    );
    assert_eq!(original, group(&catalog));
}

#[test]
fn stale_overdrawn_future_noop_and_exhausted_moves_produce_no_plan() {
    let catalog = catalog();
    let original = group(&catalog);
    let base = command(&catalog, &original, 1, 4, 8);
    for change in [
        MovementCommand {
            expected_stock_revision: revision(2),
            ..base.clone()
        },
        MovementCommand {
            expected_catalog_revision: revision(2),
            ..base.clone()
        },
        MovementCommand {
            expected_group_revision: revision(2),
            ..base.clone()
        },
        MovementCommand {
            group: id(90),
            ..base.clone()
        },
        MovementCommand {
            packs: packs(0),
            ..base.clone()
        },
        MovementCommand {
            packs: packs(21),
            ..base.clone()
        },
        MovementCommand {
            to: id(3),
            ..base.clone()
        },
        MovementCommand {
            to: id(1),
            ..base.clone()
        },
        MovementCommand {
            occurred: Occurrence::Known(at(101)),
            ..base.clone()
        },
    ] {
        assert!(
            change
                .plan(&catalog, revision(1), &original, id(30), at(100))
                .is_err()
        );
    }
    let exhausted = MovementCommand {
        expected_stock_revision: revision(HubRevision::MAXIMUM),
        ..base.clone()
    };
    assert_eq!(
        exhausted.plan(
            &catalog,
            revision(HubRevision::MAXIMUM),
            &original,
            id(30),
            at(100)
        ),
        Err(MovementError::RevisionExhausted)
    );
    let mut old = original.clone();
    old.revision = revision(HubRevision::MAXIMUM);
    let exhausted = MovementCommand {
        expected_group_revision: old.revision,
        ..base
    };
    assert_eq!(
        exhausted.plan(&catalog, revision(1), &old, id(30), at(100)),
        Err(MovementError::RevisionExhausted)
    );
    assert_eq!(original, group(&catalog));
}

#[test]
fn movement_elsewhere_or_without_a_target_does_not_complete_putaway() {
    let catalog = catalog();
    let mut original = group(&catalog);
    let elsewhere = command(&catalog, &original, 1, 5, 8)
        .plan(&catalog, revision(1), &original, id(30), at(100))
        .unwrap();
    assert!(elsewhere.work_updates.is_empty());
    assert_eq!(
        elsewhere.moved.work[0].requirement.quantity.remaining(),
        packs(20)
    );
    original.work[0].requirement.target = None;
    let unplanned = command(&catalog, &original, 1, 4, 20)
        .plan(&catalog, revision(1), &original, id(30), at(100))
        .unwrap();
    assert!(unplanned.work_updates.is_empty());
}

#[test]
fn malformed_or_mixed_work_scope_is_never_guessed() {
    let catalog = catalog();
    let original = group(&catalog);
    let mut duplicate = original.clone();
    duplicate.work.push(duplicate.work[0].clone());
    assert_eq!(duplicate.validate(), Err(MovementError::InvalidWorkScope));
    let mut wrong = original.clone();
    wrong.work[0].requirement.trace = id(99);
    assert_eq!(wrong.validate(), Err(MovementError::InvalidWorkScope));
    wrong = original.clone();
    wrong.work[0].progress = WorkProgress::Completed;
    assert_eq!(wrong.validate(), Err(MovementError::InvalidWorkScope));
    wrong = original.clone();
    wrong.location = id(4);
    assert_eq!(wrong.validate(), Err(MovementError::InvalidWorkScope));
    wrong = original.clone();
    wrong.parent = Some(id(90));
    assert_eq!(
        wrong.validate(),
        Err(MovementError::PhysicalIdentificationRequired)
    );
    wrong = original.clone();
    wrong.work = vec![wrong.work[0].clone(); HUB_GROUP_REQUIREMENTS + 1];
    assert_eq!(wrong.validate(), Err(MovementError::Capacity));
}

fn replan(catalog: &ReferenceCatalog, work: &WorkRequirement, stock: u64) -> ReplanCommand {
    ReplanCommand {
        expected_stock_revision: revision(stock),
        expected_catalog_revision: catalog.revision(),
        requirement: work.id,
        expected_work_revision: work.revision,
        target: id(5),
        observation: basis("Remaining crates require the newly confirmed B3 destination"),
    }
}

#[test]
fn replan_retires_remaining_work_without_moving_or_faking_completion() {
    let catalog = catalog();
    let original = group(&catalog);
    let moved = command(&catalog, &original, 1, 4, 8)
        .plan(&catalog, revision(1), &original, id(30), at(100))
        .unwrap();
    let pending = moved.remaining.unwrap();
    let work = pending.work[0].requirement.clone();
    let plan = replan(&catalog, &work, 2)
        .plan(
            &catalog,
            revision(2),
            &work,
            std::slice::from_ref(&pending),
            id(40),
        )
        .unwrap();
    assert_eq!(
        (
            plan.previous.quantity.completed(),
            plan.previous.quantity.no_longer_required(),
            plan.previous.quantity.remaining()
        ),
        (packs(8), packs(12), packs(0))
    );
    assert_eq!(plan.replacement.quantity, QuantityWork::pending(packs(12)));
    assert_eq!(plan.groups[0].location, pending.location);
    assert_eq!(plan.groups[0].packs, pending.packs);
    assert_eq!(
        plan.groups[0].work[0].progress,
        WorkProgress::NoLongerRequired
    );
    assert_eq!(plan.groups[0].work[1].progress, WorkProgress::Pending);
    let complete = command(&catalog, &plan.groups[0], 3, 5, 12)
        .plan(&catalog, revision(3), &plan.groups[0], id(50), at(100))
        .unwrap();
    assert_eq!(complete.work_updates.len(), 1);
    assert_eq!(complete.work_updates[0].id, id(40));
    assert_eq!(complete.work_updates[0].quantity.completed(), packs(12));
}

#[test]
fn replan_requires_all_pending_scope_and_fresh_versions() {
    let catalog = catalog();
    let original = group(&catalog);
    let work = &original.work[0].requirement;
    let base = replan(&catalog, work, 1);
    for change in [
        ReplanCommand {
            expected_stock_revision: revision(2),
            ..base.clone()
        },
        ReplanCommand {
            expected_catalog_revision: revision(2),
            ..base.clone()
        },
        ReplanCommand {
            expected_work_revision: revision(2),
            ..base.clone()
        },
        ReplanCommand {
            target: id(4),
            ..base.clone()
        },
        ReplanCommand {
            target: id(3),
            ..base.clone()
        },
    ] {
        assert!(
            change
                .plan(
                    &catalog,
                    revision(1),
                    work,
                    std::slice::from_ref(&original),
                    id(40)
                )
                .is_err()
        );
    }
    let mut subset = original.clone();
    subset.packs = packs(10);
    assert_eq!(
        base.plan(&catalog, revision(1), work, &[subset], id(40)),
        Err(MovementError::InvalidWorkScope)
    );
    assert_eq!(
        base.plan(
            &catalog,
            revision(1),
            work,
            &[original.clone(), original.clone()],
            id(40)
        ),
        Err(MovementError::InvalidWorkScope)
    );
    let mut no_target = original.clone();
    no_target.work[0].requirement.target = None;
    assert!(
        base.plan(
            &catalog,
            revision(1),
            &no_target.work[0].requirement,
            std::slice::from_ref(&no_target),
            id(40)
        )
        .is_ok()
    );
}

#[test]
fn discrepancy_resolution_retains_observations_and_has_no_stock_or_work_effect() {
    let catalog = catalog();
    let original = group(&catalog);
    let open = Discrepancy::open(
        id(50),
        original.trace,
        Some(original.id),
        basis("Manual expected count differs; review required"),
    );
    let resolved = open
        .resolve(
            revision(1),
            basis("Observation reviewed; no physical action asserted"),
        )
        .unwrap();
    assert_eq!(resolved.observation, open.observation);
    assert_eq!(resolved.trace, open.trace);
    assert!(resolved.resolution.is_some());
    assert!(open.resolution.is_none());
    assert_eq!(
        resolved.resolve(revision(2), basis("Again")),
        Err(MovementError::AlreadyResolved)
    );
    assert_eq!(
        open.resolve(revision(2), basis("Stale")),
        Err(MovementError::StaleRevision)
    );
    assert_eq!(original, group(&catalog));
}

#[test]
fn frozen_movement_and_replan_intents_bind_material_fields() {
    let catalog = catalog();
    let original = group(&catalog);
    let base = command(&catalog, &original, 1, 4, 8);
    let hub = HubId::from_bytes([80; 16]).unwrap();
    let actor =
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap();
    let hash = base.fingerprint(hub, actor, id(30));
    for change in [
        MovementCommand {
            to: id(5),
            ..base.clone()
        },
        MovementCommand {
            packs: packs(9),
            ..base.clone()
        },
        MovementCommand {
            expected_stock_revision: revision(2),
            ..base.clone()
        },
        MovementCommand {
            expected_catalog_revision: revision(2),
            ..base.clone()
        },
        MovementCommand {
            expected_group_revision: revision(2),
            ..base.clone()
        },
        MovementCommand {
            group: id(99),
            ..base.clone()
        },
        MovementCommand {
            separated_portion: None,
            ..base.clone()
        },
        MovementCommand {
            occurred: Occurrence::Known(at(90)),
            ..base.clone()
        },
        MovementCommand {
            observation: basis("Different observed move"),
            ..base.clone()
        },
    ] {
        assert_ne!(hash, change.fingerprint(hub, actor, id(30)));
    }
    assert_ne!(hash, base.fingerprint(hub, actor, id(31)));
    let replanned = replan(&catalog, &original.work[0].requirement, 1);
    assert_ne!(
        replanned.fingerprint(hub, actor, id(40)),
        ReplanCommand {
            target: id(3),
            ..replanned.clone()
        }
        .fingerprint(hub, actor, id(40))
    );
    // Frozen vectors prevent accidental changes to published canonical v1 intents.
    assert_eq!(
        hex(hash),
        "73069359aa2274689000758bd5cbaa86d85e99e90fd331ffd5e4bb6b9ad0eb5f"
    );
    assert_eq!(
        hex(replanned.fingerprint(hub, actor, id(40))),
        "ea4c3be6cfae9b8435abd8069694d80d2aae7894d35b42cb51a9c2dd7e173e8e"
    );
}

fn hex(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn explicit_new_handling_creates_pending_work_without_resetting_the_original() {
    let catalog = catalog();
    let mut original = group(&catalog);
    original.work[0].progress = WorkProgress::Completed;
    original.work[0].requirement.quantity =
        QuantityWork::restore(packs(20), packs(20), packs(0), packs(0)).unwrap();
    let request = NewWorkCommand {
        expected_stock_revision: revision(1),
        expected_catalog_revision: catalog.revision(),
        groups: vec![original.id],
        target: id(5),
        observation: basis("Explicit new handling: relocate the completed intake to B3"),
    };
    let hub = HubId::from_bytes([80; 16]).unwrap();
    let actor =
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap();
    assert_eq!(
        hex(request.fingerprint(hub, actor, id(40))),
        "e04d82090c3ac0ef3be68b71da8b9034ec6b85fecdcbdf2eed61d009f19efe88"
    );
    let plan = request
        .plan(
            &catalog,
            revision(1),
            std::slice::from_ref(&original),
            id(40),
        )
        .unwrap();
    assert_eq!(plan.requirement.quantity, QuantityWork::pending(packs(20)));
    assert_eq!(plan.groups[0].location, original.location);
    assert_eq!(plan.groups[0].packs, original.packs);
    assert_eq!(plan.groups[0].work[0], original.work[0]);
    let moved = command(&catalog, &plan.groups[0], 2, 5, 20)
        .plan(&catalog, revision(2), &plan.groups[0], id(41), at(100))
        .unwrap();
    assert_eq!(moved.work_updates.len(), 1);
    assert_eq!(moved.work_updates[0].id, id(40));
    assert_eq!(moved.work_updates[0].quantity.completed(), packs(20));
    assert_eq!(moved.moved.work[0], original.work[0]);
}

#[test]
fn new_work_rejects_guessed_scope_implicit_replanning_and_unbounded_history() {
    let catalog = catalog();
    let original = group(&catalog);
    let request = NewWorkCommand {
        expected_stock_revision: revision(1),
        expected_catalog_revision: catalog.revision(),
        groups: vec![original.id],
        target: id(5),
        observation: basis("Explicit handling request"),
    };
    assert_eq!(
        request.plan(
            &catalog,
            revision(1),
            std::slice::from_ref(&original),
            id(40)
        ),
        Err(MovementError::InvalidWorkScope)
    );
    let mut completed = original.clone();
    completed.work.clear();
    for bad in [
        NewWorkCommand {
            groups: vec![],
            ..request.clone()
        },
        NewWorkCommand {
            groups: vec![original.id, original.id],
            ..request.clone()
        },
        NewWorkCommand {
            groups: vec![id(20), id(10)],
            ..request.clone()
        },
        NewWorkCommand {
            groups: vec![id(99)],
            ..request.clone()
        },
        NewWorkCommand {
            target: original.location,
            ..request.clone()
        },
        NewWorkCommand {
            expected_stock_revision: revision(2),
            ..request.clone()
        },
        NewWorkCommand {
            expected_catalog_revision: revision(2),
            ..request.clone()
        },
    ] {
        assert!(
            bad.plan(
                &catalog,
                revision(1),
                std::slice::from_ref(&completed),
                id(40)
            )
            .is_err()
        );
    }
    let mut unit = original.work[0].clone();
    unit.progress = WorkProgress::Completed;
    unit.requirement.quantity =
        QuantityWork::restore(packs(20), packs(20), packs(0), packs(0)).unwrap();
    completed.work = (0..HUB_GROUP_REQUIREMENTS)
        .map(|n| {
            let mut allocation = unit.clone();
            allocation.requirement.id = id(u8::try_from(n + 70).unwrap());
            allocation
        })
        .collect();
    completed.validate().unwrap();
    assert_eq!(
        request.plan(&catalog, revision(1), &[completed], id(40)),
        Err(MovementError::Capacity)
    );
}

#[test]
fn work_revision_exhaustion_and_missing_scope_fail_without_mutation() {
    let catalog = catalog();
    let mut original = group(&catalog);
    original.work[0].requirement.revision = revision(HubRevision::MAXIMUM);
    assert_eq!(
        command(&catalog, &original, 1, 4, 8).plan(
            &catalog,
            revision(1),
            &original,
            id(30),
            at(100)
        ),
        Err(MovementError::RevisionExhausted)
    );
    let work = &original.work[0].requirement;
    let replanned = replan(&catalog, work, 1);
    assert_eq!(
        replanned.plan(
            &catalog,
            revision(1),
            work,
            std::slice::from_ref(&original),
            id(40)
        ),
        Err(MovementError::RevisionExhausted)
    );
    assert!(
        replanned
            .plan(&catalog, revision(1), work, &[], id(40))
            .is_err()
    );
    assert_eq!(
        original.work[0].requirement.quantity,
        QuantityWork::pending(packs(20))
    );
}

#[test]
fn handling_identities_are_frozen_namespaced_and_operation_bound() {
    let ids = HandlingIdentities::from_operation_bytes(id(60)).unwrap();
    assert_eq!(ids.group.to_hex(), "9bc42af8d5f461a27d46d62e5d58e859");
    assert_eq!(ids.requirement.to_hex(), "aa79f3318b9eb0404f1f182ba9da507d");
    assert_eq!(ids.discrepancy.to_hex(), "49d53b24b0f9d291f793c46bb0681b4e");
    assert_eq!(
        ids,
        HandlingIdentities::from_operation_bytes(id(60)).unwrap()
    );
    assert_ne!(
        ids,
        HandlingIdentities::from_operation_bytes(id(61)).unwrap()
    );
    assert_ne!(ids.group, ids.requirement);
    assert_ne!(ids.group, ids.discrepancy);
    assert_ne!(ids.requirement, ids.discrepancy);
}

#[test]
fn selected_scope_has_an_exact_finite_bound_and_checked_pack_sum() {
    let catalog = catalog();
    let template = group(&catalog);
    let mut groups: Vec<_> = (0..HUB_REQUIREMENT_GROUPS)
        .map(|index| {
            let mut group = template.clone();
            group.id = HubRecordId::from_bytes(u128::try_from(index + 1000).unwrap().to_be_bytes())
                .unwrap();
            group.parent = Some(template.id);
            group.identification = Some(PhysicalIdentification {
                label: label(&format!("Tagged group {index}")),
                observation: basis("Separate fictional tagged container"),
            });
            group.packs = packs(1);
            group.work.clear();
            group
        })
        .collect();
    let mut request = NewWorkCommand {
        expected_stock_revision: revision(1),
        expected_catalog_revision: catalog.revision(),
        groups: groups.iter().map(|group| group.id).collect(),
        target: id(5),
        observation: basis("Explicit handling of identified groups"),
    };
    let plan = request
        .plan(&catalog, revision(1), &groups, id(40))
        .unwrap();
    assert_eq!(plan.requirement.quantity.required(), packs(256));
    request.groups.push(id(250));
    assert!(request.validate_shape().is_err());
    groups.truncate(2);
    for group in &mut groups {
        group.packs = packs(600_000);
    }
    request.groups = groups.iter().map(|group| group.id).collect();
    assert!(
        request
            .plan(&catalog, revision(1), &groups, id(40))
            .is_err()
    );
    assert_eq!(groups[0].packs, packs(600_000));
    assert!(groups[0].work.is_empty());
}

#[test]
fn uncertain_movement_never_erases_known_causal_history() {
    let catalog = catalog();
    let mut source = group(&catalog);
    source.causal_floor = Some(at(90));
    let mut request = command(&catalog, &source, 1, 4, 8);
    request.occurred = Occurrence::Known(at(89));
    assert_eq!(
        request.plan(&catalog, revision(1), &source, id(30), at(100)),
        Err(MovementError::Stock(StockError::InvalidOccurrence))
    );
    request.occurred = Occurrence::Interval {
        earliest: at(85),
        latest: at(95),
    };
    let uncertain = request
        .plan(&catalog, revision(1), &source, id(30), at(100))
        .unwrap();
    assert_eq!(uncertain.moved.causal_floor, Some(at(90)));
    request.occurred = Occurrence::Unknown;
    let unknown = request
        .plan(&catalog, revision(1), &source, id(30), at(100))
        .unwrap();
    assert_eq!(unknown.moved.causal_floor, Some(at(90)));
    let mut back = command(&catalog, &unknown.moved, 2, 3, 8);
    back.occurred = Occurrence::Known(at(89));
    assert!(
        back.plan(&catalog, revision(2), &unknown.moved, id(31), at(100))
            .is_err()
    );
}
