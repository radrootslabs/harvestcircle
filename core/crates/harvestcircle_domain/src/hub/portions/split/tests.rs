use super::*;
use crate::hub::movement::{WorkAllocation, WorkProgress, WorkRequirement};
use crate::hub::reference::{HubLabel, LocationPurpose, ReferenceValue};
use crate::hub::stock::QuantityWork;

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn basis(value: &str) -> SourceProvenance {
    SourceProvenance::new(value).unwrap()
}
fn identified(label: &str) -> PhysicalIdentification {
    PhysicalIdentification {
        label: HubLabel::new(label).unwrap(),
        observation: basis("Eight separately tagged crates"),
    }
}
fn fixture() -> (ReferenceCatalog, PhysicalStockGroup, SplitCommand) {
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
    let source = PhysicalStockGroup {
        id: id(10),
        trace: id(11),
        variant: id(2),
        parent: None,
        identification: None,
        location: id(3),
        packs: packs(20),
        revision: rev(1),
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
                revision: rev(1),
            },
            progress: WorkProgress::Pending,
        }],
    };
    let command = SplitCommand {
        expected_stock_revision: rev(2),
        expected_catalog_revision: catalog.revision(),
        group: source.id,
        expected_group_revision: source.revision,
        packs: packs(8),
        identification: identified("Blue eight"),
        occurred: Occurrence::Unknown,
        observation: basis("Observed separate stack"),
    };
    (catalog, source, command)
}

#[test]
fn identified_same_location_separation_conserves_counts_origin_and_pending_work() {
    let (catalog, source, command) = fixture();
    let before = source.clone();
    let plan = command
        .plan(&catalog, rev(2), &source, id(20), at(100))
        .unwrap();
    assert_eq!(plan.stock_revision, rev(3));
    assert_eq!(plan.remainder.packs, packs(12));
    assert_eq!(plan.portion.packs, packs(8));
    assert_eq!(plan.remainder.location, source.location);
    assert_eq!(plan.portion.location, source.location);
    assert_eq!(plan.location.id, source.location);
    assert_eq!(plan.remainder.id, source.id);
    assert_eq!(plan.remainder.revision, rev(2));
    assert_eq!(plan.portion.parent, Some(source.id));
    assert_eq!(plan.portion.id, id(20));
    assert_eq!(plan.portion.revision, rev(1));
    assert_eq!(plan.portion.identification, Some(command.identification));
    assert_eq!(plan.remainder.work, source.work);
    assert_eq!(plan.portion.work, source.work);
    assert_eq!(plan.portion.trace, source.trace);
    assert_eq!(plan.portion.variant, source.variant);
    assert_eq!(source, before);
}

#[test]
fn nested_portions_keep_the_original_population_and_never_mint_work_credit() {
    let (catalog, source, command) = fixture();
    let first = command
        .plan(&catalog, rev(2), &source, id(20), at(100))
        .unwrap();
    let nested = SplitCommand {
        expected_stock_revision: first.stock_revision,
        group: first.portion.id,
        expected_group_revision: first.portion.revision,
        packs: packs(5),
        identification: identified("Red five"),
        ..command
    }
    .plan(
        &catalog,
        first.stock_revision,
        &first.portion,
        id(21),
        at(100),
    )
    .unwrap();
    assert_eq!(nested.remainder.packs, packs(3));
    assert_eq!(nested.portion.packs, packs(5));
    assert_eq!(nested.portion.parent, Some(first.portion.id));
    assert_eq!(nested.portion.trace, source.trace);
    assert_eq!(
        first
            .remainder
            .packs
            .checked_add(nested.remainder.packs)
            .unwrap()
            .checked_add(nested.portion.packs)
            .unwrap(),
        source.packs
    );
    for group in [&first.remainder, &nested.remainder, &nested.portion] {
        assert_eq!(group.work, source.work);
        assert_eq!(group.location, source.location);
    }
}

#[test]
fn completed_work_follows_each_portion_without_reset_or_another_completion() {
    let (catalog, mut source, command) = fixture();
    source.location = id(4);
    source.work[0].progress = WorkProgress::Completed;
    source.work[0].requirement.quantity =
        QuantityWork::restore(packs(20), packs(20), packs(0), packs(0)).unwrap();
    let plan = command
        .plan(&catalog, rev(2), &source, id(20), at(100))
        .unwrap();
    assert_eq!(plan.portion.work, source.work);
    assert_eq!(plan.remainder.work, source.work);
    assert_eq!(plan.portion.location, id(4));
    assert_eq!(
        plan.portion.work[0].requirement.quantity.completed(),
        packs(20)
    );
}

#[test]
fn physical_time_constrains_the_observed_portion_without_inventing_a_time_for_the_remainder() {
    let (catalog, mut source, mut command) = fixture();
    source.causal_floor = Some(at(10));
    command.occurred = Occurrence::Known(at(90));
    let plan = command
        .plan(&catalog, rev(2), &source, id(20), at(100))
        .unwrap();
    assert_eq!(plan.portion.causal_floor, Some(at(90)));
    assert_eq!(plan.remainder.causal_floor, Some(at(10)));
    command.occurred = Occurrence::Unknown;
    assert_eq!(
        command
            .plan(&catalog, rev(2), &source, id(20), at(100))
            .unwrap()
            .portion
            .causal_floor,
        Some(at(10))
    );
    command.occurred = Occurrence::Known(at(9));
    assert!(
        command
            .plan(&catalog, rev(2), &source, id(20), at(100))
            .is_err()
    );
}

#[test]
fn stale_or_indistinguishable_scope_cannot_produce_a_split_plan() {
    let (catalog, source, command) = fixture();
    for stale in [
        SplitCommand {
            expected_stock_revision: rev(3),
            ..command.clone()
        },
        SplitCommand {
            expected_catalog_revision: rev(4),
            ..command.clone()
        },
        SplitCommand {
            expected_group_revision: rev(2),
            ..command.clone()
        },
        SplitCommand {
            group: id(99),
            ..command.clone()
        },
    ] {
        assert_eq!(
            stale.plan(&catalog, rev(2), &source, id(20), at(100)),
            Err(MovementError::StaleRevision)
        );
    }
    for quantity in [0, 20, 21] {
        assert_eq!(
            SplitCommand {
                packs: packs(quantity),
                ..command.clone()
            }
            .plan(&catalog, rev(2), &source, id(20), at(100)),
            Err(MovementError::InvalidGroup)
        );
    }
    assert_eq!(
        command.plan(&catalog, rev(2), &source, source.id, at(100)),
        Err(MovementError::IdentityConflict)
    );
    let tagged = PhysicalStockGroup {
        parent: Some(id(9)),
        identification: Some(command.identification.clone()),
        ..source
    };
    assert_eq!(
        command.plan(&catalog, rev(2), &tagged, id(20), at(100)),
        Err(MovementError::IdentityConflict)
    );
}

#[test]
fn revision_exhaustion_and_quantity_limits_do_not_wrap() {
    let (catalog, mut source, mut command) = fixture();
    source.packs = packs(1_000_000);
    source.work[0].requirement.quantity = QuantityWork::pending(source.packs);
    command.packs = packs(999_999);
    let plan = command
        .plan(&catalog, rev(2), &source, id(20), at(100))
        .unwrap();
    assert_eq!(plan.remainder.packs, packs(1));
    command.expected_stock_revision = HubRevision::from_value(HubRevision::MAXIMUM).unwrap();
    assert_eq!(
        command.plan(
            &catalog,
            command.expected_stock_revision,
            &source,
            id(20),
            at(100)
        ),
        Err(MovementError::RevisionExhausted)
    );
    source.revision = command.expected_stock_revision;
    command.expected_group_revision = source.revision;
    assert_eq!(
        command.plan(
            &catalog,
            command.expected_stock_revision,
            &source,
            id(20),
            at(100)
        ),
        Err(MovementError::RevisionExhausted)
    );
}

#[test]
fn canonical_split_intent_is_bound_to_original_scope_actor_and_physical_observation() {
    let (_, _, command) = fixture();
    let hub = HubId::from_bytes([1; 16]).unwrap();
    let actor =
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap();
    let hash = command.fingerprint(hub, actor, id(20));
    let hex = hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        hex,
        "5a42117a368fa23f837d8e3ed9d9b5d3ce8e646843f9d61ef6d2a450981081f1"
    );
    let changed = SplitCommand {
        observation: basis("Different original basis"),
        ..command.clone()
    };
    assert_ne!(changed.fingerprint(hub, actor, id(20)), hash);
    assert_ne!(command.fingerprint(hub, actor, id(21)), hash);
    assert_ne!(
        command.fingerprint(HubId::from_bytes([2; 16]).unwrap(), actor, id(20)),
        hash
    );
    assert_ne!(
        command.fingerprint(
            hub,
            PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
                .unwrap(),
            id(20)
        ),
        hash
    );
}
