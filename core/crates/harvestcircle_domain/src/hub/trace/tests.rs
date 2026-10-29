use super::*;
use crate::hub::reference::{
    HubLabel, LocationPurpose, ReferenceValue, SourceCode, SourceReference, SourceReferenceKind,
};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
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
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn record(n: u8, value: ReferenceValue) -> ReferenceRecord {
    ReferenceRecord {
        id: id(n),
        revision: rev(1),
        value,
    }
}
fn label(s: &str) -> HubLabel {
    HubLabel::new(s).unwrap()
}
fn fixture() -> TraceSnapshot {
    let product = record(
        1,
        ReferenceValue::Product {
            label: label("Fictional apples"),
        },
    );
    let variant = record(
        2,
        ReferenceValue::Pack {
            label: label("Original crate"),
            product: product.id,
            nominal_grams: None,
        },
    );
    let receiving = record(
        3,
        ReferenceValue::Location {
            label: label("Receiving"),
            purpose: LocationPurpose::Receiving,
        },
    );
    let trace = ReceiptTrace {
        id: id(4),
        variant,
        product,
        supplier: None,
        producer: None,
        source: SourceAttribution::Unknown,
        source_issuer: None,
        original_packs: packs(20),
        receiving: receiving.clone(),
        planned_location: None,
        occurred: Occurrence::Unknown,
        recorded_at: at(100),
        actor: actor(),
        observation: text("Counted twenty original crates"),
    };
    TraceSnapshot {
        hub: HubId::from_bytes([1; 16]).unwrap(),
        stock_revision: rev(5),
        catalog_revision: rev(5),
        trace,
        on_hand: vec![TracePosition {
            group: id(5),
            revision: rev(2),
            packs: packs(15),
            location: receiving,
            identification: None,
            treatment: GroupTreatment {
                group: id(5),
                packs: packs(15),
                acceptance: crate::hub::treatment::AcceptanceStatus::ReviewRequired,
                acceptance_decision: None,
                active_holds: vec![],
            },
        }],
        exits: vec![TraceExit {
            group: id(6),
            packs: packs(5),
            departure: TraceDeparture {
                id: id(7),
                operation: id(8),
                stock_revision: rev(5),
                preparation: id(9),
                identification: id(10),
                recipient: record(
                    11,
                    ReferenceValue::Party {
                        label: label("Recipient A"),
                    },
                ),
                actor: actor(),
                recorded_at: at(200),
                occurred: Occurrence::Unknown,
                observation: text("Five original crates actually left"),
            },
        }],
    }
}
fn source(trace: &mut ReceiptTrace, n: u8, issuer: u8, certainty: SourceCertainty) {
    trace.source = SourceAttribution::Referenced {
        source: SourceReference {
            id: id(n),
            issuer: Some(id(issuer)),
            kind: SourceReferenceKind::SupplierReference,
            code: SourceCode::new("FX-1027").unwrap(),
            provenance: text("Original source label"),
        },
        certainty,
    };
    trace.source_issuer = Some(record(
        issuer,
        ReferenceValue::Party {
            label: label(if issuer == 21 { "Farm X" } else { "Supplier Y" }),
        },
    ));
}
#[test]
fn source_impact_keeps_unknown_and_different_possible_sources_without_merging_code_collisions() {
    let mut a = fixture().trace;
    source(&mut a, 20, 21, SourceCertainty::Established);
    let mut b = a.clone();
    b.id = id(12);
    source(&mut b, 22, 23, SourceCertainty::Established);
    let unknown = fixture().trace;
    let mut possible_b = b.clone();
    source(&mut possible_b, 22, 23, SourceCertainty::ReportedPossible);
    let scope = TraceScope::SourceImpact(id(20));
    assert!(scope.includes(&a));
    assert!(!scope.includes(&b));
    assert!(scope.includes(&unknown));
    assert!(scope.includes(&possible_b));
    let mut unidentified_issuer = b.clone();
    if let SourceAttribution::Referenced { source, .. } = &mut unidentified_issuer.source {
        source.issuer = None;
    }
    unidentified_issuer.source_issuer = None;
    assert!(scope.includes(&unidentified_issuer));
    assert_eq!(
        source_certainty_label(&possible_b.source),
        "Reported possible"
    );
    assert_eq!(source_certainty_label(&unknown.source), "Unknown");
    let exact = TraceScope::Receipt(b.id);
    assert!(exact.includes(&b));
    assert!(!exact.includes(&a));
}
#[test]
fn literal_field_search_disambiguates_issuers_but_cannot_narrow_potential_impact_scope() {
    let mut a = fixture().trace;
    source(&mut a, 20, 21, SourceCertainty::Established);
    let mut b = a.clone();
    b.id = id(12);
    source(&mut b, 22, 23, SourceCertainty::Established);
    let search = TraceSelection {
        scope: TraceScope::All,
        text: "fx-1027".into(),
    };
    assert!(search.includes(&a).unwrap());
    assert!(search.includes(&b).unwrap());
    let issuer = TraceSelection {
        scope: TraceScope::All,
        text: "farm x".into(),
    };
    assert!(issuer.includes(&a).unwrap());
    assert!(!issuer.includes(&b).unwrap());
    let literal = TraceSelection {
        scope: TraceScope::All,
        text: "%_".into(),
    };
    assert!(!literal.includes(&a).unwrap());
    let unsafe_filter = TraceSelection {
        scope: TraceScope::SourceImpact(id(20)),
        text: "Farm X".into(),
    };
    assert_eq!(unsafe_filter.validate(), Err(TraceError::InvalidQuery));
    for text in ["x".repeat(TRACE_SEARCH_BYTES + 1), "line\nbreak".into()] {
        assert_eq!(
            TraceSelection {
                scope: TraceScope::All,
                text
            }
            .validate(),
            Err(TraceError::InvalidQuery)
        );
    }
}
#[test]
fn physical_accounting_keeps_originals_depleted_populations_and_distinct_pack_units() {
    let mut snapshot = fixture();
    assert_eq!(
        snapshot.balance().unwrap(),
        TraceBalance {
            variant: id(2),
            original: packs(20),
            on_hand: packs(15),
            departed: packs(5)
        }
    );
    snapshot.on_hand.clear();
    snapshot.exits[0].packs = packs(20);
    assert_eq!(snapshot.balance().unwrap().on_hand, packs(0));
    assert_eq!(snapshot.balance().unwrap().original, packs(20));
    let mut other = snapshot.clone();
    other.trace.variant.id = id(15);
    assert_ne!(
        snapshot.balance().unwrap().variant,
        other.balance().unwrap().variant
    );
    assert!(TraceScope::All.includes(&snapshot.trace));
}
#[test]
fn impossible_totals_duplicate_groups_and_inconsistent_departure_history_fail_closed() {
    let mut snapshot = fixture();
    snapshot.on_hand[0].packs = packs(14);
    snapshot.on_hand[0].treatment.packs = packs(14);
    assert_eq!(snapshot.balance(), Err(TraceError::Accounting));
    let mut snapshot = fixture();
    snapshot.exits[0].group = snapshot.on_hand[0].group;
    assert_eq!(snapshot.balance(), Err(TraceError::InvalidSnapshot));
    let mut snapshot = fixture();
    snapshot.on_hand[0].packs = packs(0);
    assert_eq!(snapshot.balance(), Err(TraceError::InvalidSnapshot));
    let mut snapshot = fixture();
    snapshot.exits[0].departure.occurred = Occurrence::Known(at(201));
    assert_eq!(snapshot.balance(), Err(TraceError::InvalidSnapshot));
    let mut snapshot = fixture();
    let mut second = snapshot.exits[0].clone();
    second.group = id(13);
    second.departure.observation = text("Conflicting original");
    snapshot.exits.push(second);
    assert_eq!(snapshot.balance(), Err(TraceError::InvalidSnapshot));
    let mut snapshot = fixture();
    snapshot.on_hand = vec![snapshot.on_hand[0].clone(); HUB_LINEAGE_GROUPS];
    assert_eq!(snapshot.balance(), Err(TraceError::Capacity));
}
