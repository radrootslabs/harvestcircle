use super::*;
use crate::hub::reference::{HubLabel, SourceCode, SourceReference};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn key() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn now(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn label(text: &str) -> HubLabel {
    HubLabel::new(text).unwrap()
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
            ReferenceValue::Party {
                label: label("Fictional supplier"),
            },
        ),
        (
            4,
            ReferenceValue::Party {
                label: label("Fictional producer"),
            },
        ),
        (
            5,
            ReferenceValue::Party {
                label: label("Fictional issuer"),
            },
        ),
        (
            6,
            ReferenceValue::Location {
                label: label("Receiving"),
                purpose: LocationPurpose::Receiving,
            },
        ),
        (
            7,
            ReferenceValue::Location {
                label: label("A3"),
                purpose: LocationPurpose::Storage,
            },
        ),
    ] {
        catalog.create(catalog.revision(), id(n), value).unwrap();
    }
    catalog
}
fn source() -> SourceAttribution {
    SourceAttribution::Referenced {
        source: SourceReference {
            id: id(50),
            issuer: Some(id(5)),
            kind: SourceReferenceKind::ProducerBatch,
            code: SourceCode::new("BATCH-01").unwrap(),
            provenance: SourceProvenance::new("Manually read from fictional producer label")
                .unwrap(),
        },
        certainty: SourceCertainty::Established,
    }
}
fn command(catalog: &ReferenceCatalog) -> ReceiptCommand {
    ReceiptCommand {
        expected_stock_revision: HubRevision::INITIAL,
        expected_catalog_revision: catalog.revision(),
        variant: id(2),
        supplier: Some(id(3)),
        producer: Some(id(4)),
        source: source(),
        packs: packs(20),
        receiving: id(6),
        planned_location: Some(id(7)),
        occurred: Occurrence::Known(now(90)),
        observation: SourceProvenance::new("Manually counted twenty original crates at Receiving")
            .unwrap(),
    }
}
fn plan(command: &ReceiptCommand, catalog: &ReferenceCatalog) -> Result<ReceiptPlan, StockError> {
    command.plan(
        catalog,
        HubRevision::INITIAL,
        id(60),
        id(61),
        key(),
        now(100),
    )
}
fn fingerprint(command: &ReceiptCommand) -> [u8; 32] {
    command.fingerprint(HubId::from_bytes([1; 16]).unwrap(), key(), id(60), id(61))
}

#[test]
fn received_twenty_exist_only_at_receiving_with_twenty_pending_work() {
    let catalog = catalog();
    let before = catalog.clone();
    let planned = plan(&command(&catalog), &catalog).unwrap();
    assert_eq!(planned.stock_revision.value(), 2);
    assert_eq!(planned.position.location, id(6));
    assert_eq!(planned.position.packs, packs(20));
    assert_eq!(planned.trace.planned_location.unwrap().id, id(7));
    assert_ne!(planned.position.location, id(7));
    assert_eq!(planned.put_away.required(), packs(20));
    assert_eq!(planned.put_away.completed(), packs(0));
    assert_eq!(planned.put_away.no_longer_required(), packs(0));
    assert_eq!(planned.put_away.remaining(), packs(20));
    assert_eq!(planned.trace.original_packs, packs(20));
    assert_eq!(catalog, before);
}

#[test]
fn unknowns_and_possible_sources_are_preserved_without_inferred_parties_or_clocks() {
    let catalog = catalog();
    let mut input = command(&catalog);
    input.supplier = None;
    input.producer = None;
    input.source = SourceAttribution::Unknown;
    input.occurred = Occurrence::Unknown;
    input.planned_location = None;
    let result = plan(&input, &catalog).unwrap();
    assert_eq!(result.trace.source, SourceAttribution::Unknown);
    assert!(result.trace.supplier.is_none());
    assert!(result.trace.producer.is_none());
    assert!(result.trace.source_issuer.is_none());
    assert_eq!(result.trace.occurred, Occurrence::Unknown);
    assert_eq!(result.trace.recorded_at, now(100));
    assert!(result.trace.source_needs_review());
    assert_eq!(result.put_away.remaining(), packs(20));
    let SourceAttribution::Referenced { source, .. } = source() else {
        panic!()
    };
    input.source = SourceAttribution::Referenced {
        source,
        certainty: SourceCertainty::ReportedPossible,
    };
    assert!(plan(&input, &catalog).unwrap().trace.source_needs_review());
}

#[test]
fn supplier_producer_and_issuer_remain_distinct_even_with_equal_codes() {
    let catalog = catalog();
    let input = command(&catalog);
    let trace = plan(&input, &catalog).unwrap().trace;
    assert_eq!(trace.supplier.as_ref().unwrap().id, id(3));
    assert_eq!(trace.producer.as_ref().unwrap().id, id(4));
    assert_eq!(trace.source_issuer.as_ref().unwrap().id, id(5));
    assert!(!trace.source_needs_review());
    let mut other = input.source.clone();
    if let SourceAttribution::Referenced { source, .. } = &mut other {
        source.id = id(51);
        source.issuer = Some(id(3));
    }
    assert_eq!(
        validate_source_reuse(&other, [input.source.clone()]),
        Ok(())
    );
    let mut changed = input.source.clone();
    if let SourceAttribution::Referenced { source, .. } = &mut changed {
        source.issuer = Some(id(3));
    }
    assert_eq!(
        validate_source_reuse(&changed, [input.source.clone()]),
        Err(StockError::SourceIdentityConflict)
    );
    assert_eq!(
        validate_source_reuse(&input.source, [input.source.clone()]),
        Ok(())
    );
}

#[test]
fn receipt_retains_original_reference_versions_after_catalog_renaming() {
    let mut catalog = catalog();
    let input = command(&catalog);
    let result = plan(&input, &catalog).unwrap();
    let original_hash = fingerprint(&input);
    catalog
        .revise(
            catalog.revision(),
            id(1),
            HubRevision::INITIAL,
            ReferenceValue::Product {
                label: label("Current renamed apples"),
            },
        )
        .unwrap();
    assert_eq!(
        result.trace.product.value.label().as_str(),
        "Fictional apples"
    );
    assert_eq!(result.trace.variant.revision, HubRevision::INITIAL);
    assert_eq!(fingerprint(&input), original_hash);
    assert_eq!(plan(&input, &catalog), Err(StockError::StaleRevision));
}

#[test]
fn invalid_receipts_and_stale_versions_fail_without_mutating_reference_state() {
    let catalog = catalog();
    let original = catalog.clone();
    let base = command(&catalog);
    let invalid = [
        (
            ReceiptCommand {
                packs: packs(0),
                ..base.clone()
            },
            StockError::EmptyQuantity,
        ),
        (
            ReceiptCommand {
                receiving: id(7),
                ..base.clone()
            },
            StockError::InvalidReceivingLocation,
        ),
        (
            ReceiptCommand {
                planned_location: Some(id(6)),
                ..base.clone()
            },
            StockError::InvalidPutAwayTarget,
        ),
        (
            ReceiptCommand {
                variant: id(1),
                ..base.clone()
            },
            StockError::Reference(HubReferenceError::WrongReferenceKind),
        ),
        (
            ReceiptCommand {
                supplier: Some(id(6)),
                ..base.clone()
            },
            StockError::Reference(HubReferenceError::WrongReferenceKind),
        ),
        (
            ReceiptCommand {
                producer: Some(id(99)),
                ..base.clone()
            },
            StockError::Reference(HubReferenceError::MissingReference),
        ),
        (
            ReceiptCommand {
                occurred: Occurrence::Known(now(101)),
                ..base.clone()
            },
            StockError::InvalidOccurrence,
        ),
        (
            ReceiptCommand {
                expected_stock_revision: HubRevision::from_value(2).unwrap(),
                ..base.clone()
            },
            StockError::StaleRevision,
        ),
    ];
    for (input, expected) in invalid {
        assert_eq!(plan(&input, &catalog), Err(expected));
        assert_eq!(catalog, original);
    }
    let mut exhausted = base;
    exhausted.expected_stock_revision = HubRevision::from_value(HubRevision::MAXIMUM).unwrap();
    assert_eq!(
        exhausted.plan(
            &catalog,
            exhausted.expected_stock_revision,
            id(60),
            id(61),
            key(),
            now(100)
        ),
        Err(StockError::RevisionExhausted)
    );
}

#[test]
fn occurrence_and_quantity_work_cannot_fabricate_order_or_progress() {
    assert_eq!(
        Occurrence::Interval {
            earliest: now(50),
            latest: now(90)
        }
        .validate(now(100)),
        Ok(())
    );
    assert_eq!(
        Occurrence::Interval {
            earliest: now(90),
            latest: now(50)
        }
        .validate(now(100)),
        Err(StockError::InvalidOccurrence)
    );
    assert_eq!(
        Occurrence::Interval {
            earliest: now(50),
            latest: now(101)
        }
        .validate(now(100)),
        Err(StockError::InvalidOccurrence)
    );
    assert_eq!(
        QuantityWork::restore(packs(20), packs(8), packs(5), packs(7))
            .unwrap()
            .remaining(),
        packs(7)
    );
    assert_eq!(
        QuantityWork::restore(packs(20), packs(8), packs(5), packs(8)),
        Err(StockError::InvalidWorkAccounting)
    );
    assert!(QuantityWork::restore(packs(1_000_000), packs(1_000_000), packs(1), packs(0)).is_err());
}

#[test]
fn original_intent_hash_covers_material_fields_and_has_a_stable_vector() {
    let catalog = catalog();
    let input = command(&catalog);
    let hash = fingerprint(&input);
    assert_eq!(
        hash.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "8821e3cf33231c5f9c093b130f441957f5f0f72a21d107217591eb0b8c0dcae1",
    );
    let changes = [
        ReceiptCommand {
            expected_stock_revision: HubRevision::from_value(2).unwrap(),
            ..input.clone()
        },
        ReceiptCommand {
            expected_catalog_revision: HubRevision::INITIAL,
            ..input.clone()
        },
        ReceiptCommand {
            variant: id(3),
            ..input.clone()
        },
        ReceiptCommand {
            supplier: None,
            ..input.clone()
        },
        ReceiptCommand {
            producer: None,
            ..input.clone()
        },
        ReceiptCommand {
            source: SourceAttribution::Unknown,
            ..input.clone()
        },
        ReceiptCommand {
            packs: packs(19),
            ..input.clone()
        },
        ReceiptCommand {
            receiving: id(7),
            ..input.clone()
        },
        ReceiptCommand {
            planned_location: None,
            ..input.clone()
        },
        ReceiptCommand {
            occurred: Occurrence::Unknown,
            ..input.clone()
        },
        ReceiptCommand {
            occurred: Occurrence::Interval {
                earliest: now(80),
                latest: now(90),
            },
            ..input.clone()
        },
        ReceiptCommand {
            observation: SourceProvenance::new("Another observation").unwrap(),
            ..input.clone()
        },
    ];
    for changed in changes {
        assert_ne!(fingerprint(&changed), hash);
    }
    let SourceAttribution::Referenced { source, certainty } = &input.source else {
        panic!()
    };
    let alternatives = [
        SourceReference {
            id: id(51),
            ..source.clone()
        },
        SourceReference {
            issuer: None,
            ..source.clone()
        },
        SourceReference {
            kind: SourceReferenceKind::Other,
            ..source.clone()
        },
        SourceReference {
            code: SourceCode::new("OTHER").unwrap(),
            ..source.clone()
        },
        SourceReference {
            provenance: SourceProvenance::new("Other original observation").unwrap(),
            ..source.clone()
        },
    ];
    for changed in alternatives {
        let changed = ReceiptCommand {
            source: SourceAttribution::Referenced {
                source: changed,
                certainty: *certainty,
            },
            ..input.clone()
        };
        assert_ne!(fingerprint(&changed), hash);
    }
    let possible = ReceiptCommand {
        source: SourceAttribution::Referenced {
            source: source.clone(),
            certainty: SourceCertainty::ReportedPossible,
        },
        ..input.clone()
    };
    assert_ne!(fingerprint(&possible), hash);
    let other_actor =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    assert_ne!(
        input.fingerprint(
            HubId::from_bytes([1; 16]).unwrap(),
            other_actor,
            id(60),
            id(61)
        ),
        hash
    );
    assert_ne!(
        input.fingerprint(HubId::from_bytes([2; 16]).unwrap(), key(), id(60), id(61)),
        hash
    );
    assert_ne!(
        input.fingerprint(HubId::from_bytes([1; 16]).unwrap(), key(), id(62), id(61)),
        hash
    );
    assert_ne!(
        input.fingerprint(HubId::from_bytes([1; 16]).unwrap(), key(), id(60), id(62)),
        hash
    );
}

#[test]
fn receipt_identities_are_stable_namespaced_and_bound_to_the_original_operation() {
    let first = ReceiptIdentities::from_operation_bytes(id(60)).unwrap();
    assert_eq!(first.trace.to_hex(), "2f54bfea8322b0d58cef5a3478ceee6f");
    assert_eq!(first.group.to_hex(), "5b3ec50b5afed4fdbb89fda99999f3e9");
    assert_eq!(first.source.to_hex(), "0e86c7ef7d9281d7bd769bb450c54312");
    assert_eq!(
        first,
        ReceiptIdentities::from_operation_bytes(id(60)).unwrap()
    );
    assert_ne!(first.trace, first.group);
    assert_ne!(first.group, first.source);
    assert_ne!(first.trace, first.source);
    assert_ne!(
        first,
        ReceiptIdentities::from_operation_bytes(id(61)).unwrap()
    );
}
