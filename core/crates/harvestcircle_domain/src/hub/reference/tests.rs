use super::*;

fn id(value: u16) -> HubRecordId {
    let mut bytes = [0; 16];
    bytes[14..].copy_from_slice(&value.to_be_bytes());
    HubRecordId::from_bytes(bytes).unwrap()
}

fn label(text: &str) -> HubLabel {
    HubLabel::new(text).unwrap()
}
fn product(text: &str) -> ReferenceValue {
    ReferenceValue::Product { label: label(text) }
}
fn party(text: &str) -> ReferenceValue {
    ReferenceValue::Party { label: label(text) }
}
fn create(
    catalog: &mut ReferenceCatalog,
    identifier: u16,
    value: ReferenceValue,
) -> ReferenceChange {
    catalog
        .create(catalog.revision(), id(identifier), value)
        .unwrap()
}

#[test]
fn record_ids_are_bounded_canonical_and_nonzero() {
    let original = id(23);
    assert_eq!(HubRecordId::from_hex(&original.to_hex()), Ok(original));
    for invalid in [
        "",
        "00000000000000000000000000000000",
        "0000000000000000000000000000000A",
        "../normal",
        "éééééééééééééééé",
    ] {
        assert_eq!(
            HubRecordId::from_hex(invalid),
            Err(HubReferenceError::InvalidId)
        );
    }
}

#[test]
fn text_uses_utf8_bounds_preserves_originals_and_rejects_controls() {
    let exact = "é".repeat(HUB_LABEL_UTF8_BYTES / 2);
    assert_eq!(HubLabel::new(&exact).unwrap().as_str(), exact);
    assert_eq!(
        HubLabel::new(format!("{exact}x")),
        Err(HubReferenceError::InvalidText)
    );
    assert_eq!(
        SourceCode::new("  Batch A  ").unwrap().as_str(),
        "  Batch A  "
    );
    for invalid in [
        "",
        " \t",
        "line\nbreak",
        "nul\0byte",
        "left\u{202e}right",
        "left\u{2066}right",
    ] {
        assert_eq!(HubLabel::new(invalid), Err(HubReferenceError::InvalidText));
    }
    assert!(SourceCode::new("x".repeat(HUB_SOURCE_CODE_UTF8_BYTES)).is_ok());
    assert!(SourceCode::new("x".repeat(HUB_SOURCE_CODE_UTF8_BYTES + 1)).is_err());
    assert!(SourceProvenance::new("x".repeat(HUB_PROVENANCE_UTF8_BYTES)).is_ok());
    assert!(SourceProvenance::new("x".repeat(HUB_PROVENANCE_UTF8_BYTES + 1)).is_err());
}

#[test]
fn whole_packs_use_checked_arithmetic_and_never_combine_unlike_variants() {
    let zero = WholePacks::from_value(0).unwrap();
    let one = WholePacks::from_value(1).unwrap();
    let max = WholePacks::from_value(HUB_MAX_WHOLE_PACKS).unwrap();
    assert_eq!(max.checked_add(zero), Ok(max));
    assert_eq!(
        max.checked_add(one),
        Err(HubReferenceError::InvalidQuantity)
    );
    assert_eq!(
        zero.checked_sub(one),
        Err(HubReferenceError::InvalidQuantity)
    );
    assert!(WholePacks::from_value(u32::MAX).is_err());
    let first = VariantQuantity {
        variant: id(1),
        packs: one,
    };
    assert_eq!(
        first.checked_add(VariantQuantity {
            variant: id(2),
            packs: one
        }),
        Err(HubReferenceError::DifferentVariant)
    );
    assert_eq!(first.checked_add(first).unwrap().packs.value(), 2);
}

#[test]
fn nominal_mass_is_optional_bounded_metadata() {
    assert_eq!(
        NominalGrams::from_value(0),
        Err(HubReferenceError::InvalidQuantity)
    );
    assert_eq!(
        NominalGrams::from_value(HUB_MAX_NOMINAL_GRAMS + 1),
        Err(HubReferenceError::InvalidQuantity)
    );
    assert_eq!(
        NominalGrams::from_value(HUB_MAX_NOMINAL_GRAMS)
            .unwrap()
            .value(),
        HUB_MAX_NOMINAL_GRAMS
    );
    let mut catalog = ReferenceCatalog::default();
    create(&mut catalog, 1, product("Apples"));
    create(
        &mut catalog,
        2,
        ReferenceValue::Pack {
            product: id(1),
            label: label("Original crate; mass unknown"),
            nominal_grams: None,
        },
    );
    assert_eq!(
        catalog
            .require_kind(id(2), ReferenceKind::Pack)
            .unwrap()
            .revision,
        HubRevision::INITIAL
    );
}

#[test]
fn changed_pack_definition_requires_another_identity() {
    let mut catalog = ReferenceCatalog::default();
    create(&mut catalog, 1, product("Apples"));
    let original = create(
        &mut catalog,
        2,
        ReferenceValue::Pack {
            product: id(1),
            label: label("10 kg crate"),
            nominal_grams: Some(NominalGrams::from_value(10_000).unwrap()),
        },
    );
    let replacement = ReferenceValue::Pack {
        product: id(1),
        label: label("5 kg crate"),
        nominal_grams: Some(NominalGrams::from_value(5_000).unwrap()),
    };
    let before = catalog.clone();
    assert_eq!(
        catalog.revise(
            catalog.revision(),
            id(2),
            HubRevision::INITIAL,
            replacement.clone()
        ),
        Err(HubReferenceError::ImmutablePack)
    );
    assert_eq!(catalog, before);
    let new = create(&mut catalog, 3, replacement);
    assert_ne!(original.current.id, new.current.id);
    assert_eq!(catalog.get(id(2)).unwrap(), &original.current);
}

#[test]
fn current_label_revision_retains_the_complete_original_version() {
    let mut catalog = ReferenceCatalog::default();
    let original = create(&mut catalog, 1, party("Fictional orchard"));
    let change = catalog
        .revise(
            catalog.revision(),
            id(1),
            HubRevision::INITIAL,
            party("Fictional Orchard Cooperative"),
        )
        .unwrap();
    assert_eq!(change.previous, Some(original.current));
    assert_eq!(change.current.id, id(1));
    assert_eq!(change.current.revision.value(), 2);
    assert_eq!(
        catalog.get(id(1)).unwrap().value.label().as_str(),
        "Fictional Orchard Cooperative"
    );
}

#[test]
fn stale_forms_duplicate_ids_and_kind_changes_do_not_mutate_catalog() {
    let mut catalog = ReferenceCatalog::default();
    create(&mut catalog, 1, party("Supplier"));
    let before = catalog.clone();
    assert_eq!(
        catalog.create(HubRevision::INITIAL, id(2), party("Other")),
        Err(HubReferenceError::StaleRevision)
    );
    assert_eq!(
        catalog.create(catalog.revision(), id(1), party("Other")),
        Err(HubReferenceError::DuplicateId)
    );
    assert_eq!(
        catalog.revise(
            catalog.revision(),
            id(1),
            HubRevision::from_value(2).unwrap(),
            party("Other")
        ),
        Err(HubReferenceError::StaleRevision)
    );
    assert_eq!(
        catalog.revise(
            catalog.revision(),
            id(1),
            HubRevision::INITIAL,
            product("Other")
        ),
        Err(HubReferenceError::WrongReferenceKind)
    );
    assert_eq!(
        catalog.revise(
            catalog.revision(),
            id(1),
            HubRevision::INITIAL,
            party("Supplier")
        ),
        Err(HubReferenceError::NoChange)
    );
    assert_eq!(catalog, before);
}

#[test]
fn location_purpose_cannot_rewrite_historical_receiving_meaning() {
    let mut catalog = ReferenceCatalog::default();
    create(
        &mut catalog,
        1,
        ReferenceValue::Location {
            label: label("Receiving"),
            purpose: LocationPurpose::Receiving,
        },
    );
    let before = catalog.clone();
    assert_eq!(
        catalog.revise(
            catalog.revision(),
            id(1),
            HubRevision::INITIAL,
            ReferenceValue::Location {
                label: label("A3"),
                purpose: LocationPurpose::Storage
            }
        ),
        Err(HubReferenceError::ImmutableLocationPurpose)
    );
    assert_eq!(catalog, before);
    assert!(
        catalog
            .revise(
                catalog.revision(),
                id(1),
                HubRevision::INITIAL,
                ReferenceValue::Location {
                    label: label("Receiving bay"),
                    purpose: LocationPurpose::Receiving
                }
            )
            .is_ok()
    );
}

#[test]
fn exact_capacity_and_revision_exhaustion_fail_without_partial_changes() {
    let mut catalog = ReferenceCatalog::default();
    for number in 1..=HUB_REFERENCE_CAPACITY {
        create(
            &mut catalog,
            u16::try_from(number).unwrap(),
            party("Fictional party"),
        );
    }
    let before = catalog.clone();
    assert_eq!(
        catalog.create(catalog.revision(), id(999), party("One too many")),
        Err(HubReferenceError::Capacity)
    );
    assert_eq!(catalog, before);
    let mut exhausted = ReferenceCatalog::restore(
        HubRevision::from_value(HubRevision::MAXIMUM).unwrap(),
        vec![],
    )
    .unwrap();
    let before = exhausted.clone();
    assert_eq!(
        exhausted.create(exhausted.revision(), id(1), party("One")),
        Err(HubReferenceError::RevisionExhausted)
    );
    assert_eq!(exhausted, before);
}

#[test]
fn restoring_current_records_rejects_duplicates_dangling_links_and_wrong_kinds() {
    let party = ReferenceRecord {
        id: id(1),
        revision: HubRevision::INITIAL,
        value: party("Supplier"),
    };
    assert_eq!(
        ReferenceCatalog::restore(HubRevision::INITIAL, vec![party.clone(), party.clone()]),
        Err(HubReferenceError::DuplicateId)
    );
    let pack = ReferenceRecord {
        id: id(2),
        revision: HubRevision::INITIAL,
        value: ReferenceValue::Pack {
            product: id(1),
            label: label("Crate"),
            nominal_grams: None,
        },
    };
    assert_eq!(
        ReferenceCatalog::restore(HubRevision::INITIAL, vec![pack.clone()]),
        Err(HubReferenceError::MissingReference)
    );
    assert_eq!(
        ReferenceCatalog::restore(HubRevision::INITIAL, vec![pack, party.clone()]),
        Err(HubReferenceError::WrongReferenceKind)
    );
    assert_eq!(
        ReferenceCatalog::restore(
            HubRevision::INITIAL,
            vec![ReferenceRecord {
                revision: HubRevision::from_value(2).unwrap(),
                ..party
            }]
        ),
        Err(HubReferenceError::StaleRevision)
    );
}

#[test]
fn same_source_code_with_another_issuer_or_unknown_identity_does_not_merge() {
    let mut catalog = ReferenceCatalog::default();
    create(&mut catalog, 1, party("Supplier"));
    create(&mut catalog, 2, party("Producer"));
    create(&mut catalog, 3, party("Issuer"));
    let original = SourceReference {
        id: id(4),
        issuer: Some(id(3)),
        kind: SourceReferenceKind::ProducerBatch,
        code: SourceCode::new("BATCH-1").unwrap(),
        provenance: SourceProvenance::new("Reported on fictional supplier slip").unwrap(),
    };
    original.validate(&catalog).unwrap();
    assert_ne!(original.issuer, Some(id(1)));
    assert_ne!(original.issuer, Some(id(2)));
    assert_ne!(
        original,
        SourceReference {
            issuer: Some(id(2)),
            ..original.clone()
        }
    );
    assert_ne!(
        SourceReference {
            issuer: None,
            ..original.clone()
        },
        SourceReference {
            id: id(5),
            issuer: None,
            ..original.clone()
        }
    );
    let possible = SourceAttribution::Referenced {
        source: original.clone(),
        certainty: SourceCertainty::ReportedPossible,
    };
    let established = SourceAttribution::Referenced {
        source: original.clone(),
        certainty: SourceCertainty::Established,
    };
    assert_ne!(possible, established);
    assert_ne!(possible, SourceAttribution::Unknown);
    create(&mut catalog, 6, product("Apples"));
    assert_eq!(
        SourceReference {
            issuer: Some(id(6)),
            ..original
        }
        .validate(&catalog),
        Err(HubReferenceError::WrongReferenceKind)
    );
}
