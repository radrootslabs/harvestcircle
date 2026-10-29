use super::*;
use crate::hub::reference::{HubLabel, LocationPurpose, SourceCode, SourceReference};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
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
fn fixture() -> (ReferenceCatalog, OpeningCommand, OpeningIdentities) {
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
                label: HubLabel::new("Already at A3").unwrap(),
                purpose: LocationPurpose::Storage,
            },
        ),
        (
            4,
            ReferenceValue::Party {
                label: HubLabel::new("Observed source issuer").unwrap(),
            },
        ),
    ] {
        catalog.create(catalog.revision(), id(n), value).unwrap();
    }
    let command = OpeningCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: catalog.revision(),
        variant: id(2),
        location: id(3),
        packs: packs(20),
        identification: PhysicalIdentification {
            label: HubLabel::new("Opening stack").unwrap(),
            observation: text("Twenty presently counted, separately tagged original crates"),
        },
        source: SourceAttribution::Unknown,
        history: OpeningHistory::Unknown,
        counted: Occurrence::Known(at(80)),
        observation: text(
            "Counted opening only; previous receipt and supplier history are unknown",
        ),
    };
    (
        catalog,
        command,
        OpeningIdentities::from_operation_bytes(id(20)).unwrap(),
    )
}

#[test]
fn counted_opening_at_storage_is_not_a_receipt_or_invented_completed_work() {
    let (catalog, command, ids) = fixture();
    let plan = command
        .plan(&catalog, HubRevision::INITIAL, ids, actor(), at(100))
        .unwrap();
    assert_eq!(
        plan.population.origin,
        PopulationOrigin::CountedOpening {
            history: OpeningHistory::Unknown
        }
    );
    assert_eq!(plan.population.initial_location.id, id(3));
    assert_eq!(plan.group.location, id(3));
    assert_eq!(plan.group.packs, packs(20));
    assert_eq!(plan.population.supplier, None);
    assert_eq!(plan.population.producer, None);
    assert_eq!(plan.population.source, SourceAttribution::Unknown);
    assert!(plan.population.source_needs_review());
    assert_eq!(plan.acceptance, AcceptanceStatus::ReviewRequired);
    assert!(plan.group.work.is_empty());
    assert_eq!(plan.population.planned_location, None);
    assert_eq!(plan.group.causal_floor, Some(at(80)));
    plan.group.validate_catalog(&catalog).unwrap();
}

#[test]
fn present_source_label_and_earlier_history_are_independent_observations() {
    let (catalog, mut command, ids) = fixture();
    command.source = SourceAttribution::Referenced {
        source: SourceReference {
            id: ids.source,
            issuer: Some(id(4)),
            kind: SourceReferenceKind::ProducerBatch,
            code: SourceCode::new("OBS-20").unwrap(),
            provenance: text("Present original label manually observed"),
        },
        certainty: SourceCertainty::Established,
    };
    let plan = command
        .plan(&catalog, HubRevision::INITIAL, ids, actor(), at(100))
        .unwrap();
    assert_eq!(
        plan.population.origin,
        PopulationOrigin::CountedOpening {
            history: OpeningHistory::Unknown
        }
    );
    assert!(!plan.population.source_needs_review());
    assert_eq!(plan.population.source_issuer.as_ref().unwrap().id, id(4));
    assert_eq!(plan.population.supplier, None);
    assert_eq!(plan.population.producer, None);
    assert_eq!(plan.acceptance, AcceptanceStatus::ReviewRequired);
}

#[test]
fn an_earlier_external_reference_is_not_an_arrival_time_or_acceptance() {
    let (catalog, mut command, ids) = fixture();
    command.history = OpeningHistory::ExternalReference(text(
        "External paper register note 7; intake time unverified",
    ));
    let plan = command
        .plan(&catalog, HubRevision::INITIAL, ids, actor(), at(100))
        .unwrap();
    assert_eq!(
        plan.population.origin,
        PopulationOrigin::CountedOpening {
            history: command.history.clone()
        }
    );
    assert_eq!(plan.population.occurred, Occurrence::Known(at(80)));
    assert_eq!(plan.population.recorded_at, at(100));
    assert_eq!(plan.acceptance, AcceptanceStatus::ReviewRequired);
}

#[test]
fn zero_opening_and_unknown_future_or_interval_count_times_are_not_admitted() {
    let (catalog, mut command, ids) = fixture();
    command.packs = packs(0);
    assert_eq!(
        command.plan(&catalog, HubRevision::INITIAL, ids, actor(), at(100)),
        Err(OpeningError::EmptyQuantity)
    );
    command.packs = packs(20);
    for counted in [
        Occurrence::Unknown,
        Occurrence::Interval {
            earliest: at(70),
            latest: at(80),
        },
    ] {
        command.counted = counted;
        assert_eq!(
            command.plan(&catalog, HubRevision::INITIAL, ids, actor(), at(100)),
            Err(OpeningError::CountTimeRequired)
        );
    }
    command.counted = Occurrence::Known(at(101));
    assert!(
        command
            .plan(&catalog, HubRevision::INITIAL, ids, actor(), at(100))
            .is_err()
    );
}

#[test]
fn wrong_reference_kinds_stale_context_and_identity_aliases_are_rejected() {
    let (catalog, mut command, ids) = fixture();
    command.location = id(4);
    assert!(
        command
            .plan(&catalog, HubRevision::INITIAL, ids, actor(), at(100))
            .is_err()
    );
    command.location = id(3);
    command.expected_stock_revision = HubRevision::from_value(2).unwrap();
    assert_eq!(
        command.plan(&catalog, HubRevision::INITIAL, ids, actor(), at(100)),
        Err(OpeningError::StaleRevision)
    );
    command.expected_stock_revision = HubRevision::INITIAL;
    assert_eq!(
        command.plan(
            &catalog,
            HubRevision::INITIAL,
            OpeningIdentities {
                group: ids.population,
                ..ids
            },
            actor(),
            at(100)
        ),
        Err(OpeningError::InvalidIdentity)
    );
}

#[test]
fn identities_are_repeatable_distinct_and_separate_from_actual_receipt_identities() {
    let (_, _, ids) = fixture();
    assert_eq!(
        ids,
        OpeningIdentities::from_operation_bytes(id(20)).unwrap()
    );
    assert_ne!(ids.population, ids.group);
    assert_ne!(ids.population, ids.source);
    assert_ne!(ids.group, ids.source);
    let receipt = ReceiptIdentities::from_operation_bytes(id(20)).unwrap();
    assert_ne!(ids.population, receipt.trace);
    assert_ne!(ids.group, receipt.group);
    assert_ne!(ids.source, receipt.source);
}

#[test]
fn opening_intent_binds_actual_location_timing_identification_and_history() {
    let (_, command, ids) = fixture();
    let original = command.fingerprint(hub(), actor(), ids);
    let mut changed = command.clone();
    changed.history =
        OpeningHistory::ExternalReference(text("A different original-history statement"));
    assert_ne!(original, changed.fingerprint(hub(), actor(), ids));
    changed = command.clone();
    changed.location = id(4);
    assert_ne!(original, changed.fingerprint(hub(), actor(), ids));
    changed = command.clone();
    changed.counted = Occurrence::Known(at(81));
    assert_ne!(original, changed.fingerprint(hub(), actor(), ids));
    changed = command.clone();
    changed.identification.label = HubLabel::new("Different physical stack").unwrap();
    assert_ne!(original, changed.fingerprint(hub(), actor(), ids));
}
