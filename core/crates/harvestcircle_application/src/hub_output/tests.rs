use super::*;
use harvestcircle_domain::hub::dispatch::{
    DispatchLineSnapshot, DispatchMaterial, DispatchStamp, IdentificationStatus,
};
use harvestcircle_domain::hub::reference::{
    HubLabel, LocationPurpose, ReferenceValue, SampleHandlingBasis, SourceProvenance, WholePacks,
};
use harvestcircle_domain::hub::trace::TracePosition;
use harvestcircle_domain::hub::{HubId, HubRevision};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn rev(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
fn label(s: &str) -> HubLabel {
    HubLabel::new(s).unwrap()
}
fn reference(n: u8, value: ReferenceValue) -> ReferenceRecord {
    ReferenceRecord {
        id: id(n),
        revision: rev(1),
        value,
    }
}
fn fixture() -> DispatchRecord {
    let actor =
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap();
    let product = reference(
        1,
        ReferenceValue::Product {
            label: label("Fictional apples & pears"),
        },
    );
    let variant = reference(
        2,
        ReferenceValue::Pack {
            label: label("Original <crate>"),
            product: product.id,
            nominal_grams: None,
        },
    );
    let receiving = reference(
        3,
        ReferenceValue::Location {
            label: label("Receiving"),
            purpose: LocationPurpose::Receiving,
        },
    );
    let recipient = reference(
        4,
        ReferenceValue::Party {
            label: label("Recipient 'A' <script>alert(1)</script>"),
        },
    );
    let trace = StockPopulation {
        id: id(5),
        product: product.clone(),
        variant: variant.clone(),
        supplier: None,
        producer: None,
        source: SourceAttribution::Unknown,
        source_issuer: None,
        original_packs: WholePacks::from_value(20).unwrap(),
        origin: harvestcircle_domain::hub::stock::PopulationOrigin::Receipt,
        initial_location: receiving.clone(),
        planned_location: None,
        occurred: Occurrence::Unknown,
        recorded_at: at(100),
        actor,
        observation: note("Original count, not a measured mass"),
    };
    let material = DispatchMaterial {
        stock_revision: rev(2),
        catalog_revision: rev(5),
        basis: SampleHandlingBasis::WHOLE_PACK_V1,
        recipient,
        lines: vec![DispatchLineSnapshot {
            group: id(6),
            group_revision: rev(1),
            packs: trace.original_packs,
            identification: None,
            location: receiving,
            product,
            variant,
            trace,
        }],
        observation: note(
            "Label <img src=\"https://example.invalid/remote\" onerror='run()'> reviewed",
        ),
    };
    let stamp = DispatchStamp {
        operation: id(7),
        stock_revision: rev(3),
        actor,
        recorded_at: at(200),
        observation: note("Reviewed original facts"),
    };
    DispatchRecord {
        id: id(8),
        hub: HubId::from_bytes([1; 16]).unwrap(),
        revision: rev(2),
        created: stamp.clone(),
        updated: stamp.clone(),
        draft: None,
        versions: vec![IdentificationVersion {
            id: id(9),
            number: 1,
            previous: None,
            material,
            issued: stamp,
            status: IdentificationStatus::Issued,
            closed: None,
        }],
    }
}
fn snapshot() -> TraceSnapshot {
    let record = fixture();
    let line = &record.versions[0].material.lines[0];
    TraceSnapshot {
        count_losses: vec![],
        hub: record.hub,
        stock_revision: rev(3),
        catalog_revision: rev(5),
        trace: line.trace.clone(),
        on_hand: vec![TracePosition {
            group: line.group,
            revision: line.group_revision,
            packs: line.packs,
            location: line.location.clone(),
            identification: None,
            treatment: harvestcircle_domain::hub::treatment::GroupTreatment {
                group: line.group,
                packs: line.packs,
                acceptance: harvestcircle_domain::hub::treatment::AcceptanceStatus::ReviewRequired,
                acceptance_decision: None,
                active_holds: vec![],
            },
        }],
        exits: vec![],
    }
}
#[test]
fn as_issued_rendering_is_stable_across_mutable_status_later_versions_and_reference_changes() {
    let mut record = fixture();
    let original = identification_output(&record, id(9)).unwrap();
    record.revision = rev(5);
    record.versions[0].status = IdentificationStatus::Superseded;
    record.versions[0].closed = Some(record.updated.clone());
    let mut next = record.versions[0].clone();
    next.id = id(10);
    next.number = 2;
    next.previous = Some(id(9));
    next.material.recipient.value = ReferenceValue::Party {
        label: label("Changed recipient B"),
    };
    record.draft = Some(next.material.clone());
    record.versions.push(next);
    assert_eq!(original, identification_output(&record, id(9)).unwrap());
    assert!(!original.html.contains("Changed recipient B"));
    assert_ne!(
        original.identity,
        identification_output(&record, id(10)).unwrap().identity
    );
    assert_eq!(
        identification_output(&record, id(11)),
        Err(LocalOutputError::InvalidVersion)
    );
}
#[test]
fn untrusted_text_is_data_without_active_or_remote_document_content() {
    let output = identification_output(&fixture(), id(9)).unwrap();
    assert!(
        output
            .html
            .contains("&lt;script&gt;alert(1)&lt;/script&gt;")
    );
    assert!(output.html.contains(
        "&lt;img src=&quot;https://example.invalid/remote&quot; onerror=&#39;run()&#39;&gt;"
    ));
    assert!(output.html.contains("apples &amp; pears"));
    for active in ["<script", "<img", "<iframe", "<a ", "<link", "<form"] {
        assert!(!output.html.contains(active));
    }
    assert!(output.html.contains("default-src 'none'"));
    assert!(output.html.contains("1970-01-01T00:03:20Z"));
    assert!(
        output
            .html
            .contains("Unknown; recording time does not establish actual time")
    );
    assert!(output.html.len() <= LOCAL_OUTPUT_MAX_BYTES);
    assert_eq!(output, identification_output(&fixture(), id(9)).unwrap());
}
#[test]
fn trace_output_exposes_recorded_scope_uncertainty_and_exact_pack_accounting() {
    let snapshot = snapshot();
    let output = trace_output(&snapshot).unwrap();
    assert!(
        output
            .html
            .contains("Unknown origin is not evidence of no relationship")
    );
    assert!(output.html.contains("not a complete action-history export"));
    assert!(output.html.contains("not establish external delivery"));
    assert!(output.html.contains("On hand:</strong> 20"));
    assert!(output.html.contains("Actual departed:</strong> 0"));
    assert!(!output.html.contains("measured mass:</strong>"));
    let mut broken = snapshot.clone();
    broken.on_hand.clear();
    assert_eq!(
        trace_output(&broken),
        Err(LocalOutputError::InvalidSnapshot)
    );
    let mut changed = snapshot.clone();
    changed.stock_revision = rev(4);
    assert_ne!(output.identity, trace_output(&changed).unwrap().identity);
}
#[test]
fn output_bounds_apply_after_escaping_and_unsupported_times_never_panic() {
    let mut html = Html(String::new());
    assert_eq!(
        html.text(&"&".repeat(LOCAL_OUTPUT_MAX_BYTES / 5 + 1)),
        Err(LocalOutputError::Capacity)
    );
    assert!(html.0.len() <= LOCAL_OUTPUT_MAX_BYTES);
    assert_eq!(utc(at(i64::MAX)), Err(LocalOutputError::InvalidTime));
    assert_eq!(utc(at(0)).unwrap(), "1970-01-01T00:00:00Z");
    assert_eq!(
        operation_text(id(1)),
        "01010101-0101-0101-0101-010101010101"
    );
}

#[test]
fn counted_opening_output_labels_current_count_and_escapes_unverified_earlier_history() {
    use harvestcircle_domain::hub::stock::{OpeningHistory, PopulationOrigin};
    let mut snapshot = snapshot();
    let receipt = trace_output(&snapshot).unwrap();
    snapshot.trace.origin = PopulationOrigin::CountedOpening {
        history: OpeningHistory::ExternalReference(note(
            "Unverified register <entry> & unknown earlier arrival",
        )),
    };
    let counted = trace_output(&snapshot).unwrap();
    assert_ne!(receipt.identity, counted.identity);
    assert!(counted.html.contains("Counted opening"));
    assert!(counted.html.contains("not a historical receipt"));
    assert!(
        counted
            .html
            .contains("Unverified register &lt;entry&gt; &amp; unknown earlier arrival")
    );
    snapshot.trace.origin = PopulationOrigin::CountedSurplus {
        count: id(40),
        related_population: id(41),
    };
    let surplus = trace_output(&snapshot).unwrap();
    assert!(surplus.html.contains("surplus of unknown origin"));
    assert!(
        surplus
            .html
            .contains("does not establish the surplus source")
    );
}

#[test]
fn trace_output_preserves_count_shortages_without_inventing_departure_and_escapes_review_text() {
    use harvestcircle_domain::hub::trace::TraceCountLoss;
    let mut value = snapshot();
    value.on_hand[0].packs = WholePacks::from_value(18).unwrap();
    value.on_hand[0].treatment.packs = value.on_hand[0].packs;
    let operation = id(12);
    value.count_losses.push(TraceCountLoss {
        reconciliation: harvestcircle_domain::hub::counts::count_reconciliation_id(operation)
            .unwrap(),
        operation,
        count: id(13),
        population: value.trace.id,
        group: value.on_hand[0].group,
        stock_revision: value.stock_revision,
        packs: WholePacks::from_value(2).unwrap(),
        confirmed_on_hand_at: at(150),
        recorded_at: at(200),
        actor: value.trace.actor,
        reason: note("Reviewed <script>unsafe()</script> & counted"),
    });
    let output = trace_output(&value).unwrap();
    assert!(output.html.contains("Unexplained count shortage"));
    assert!(output.html.contains("loss time and cause unknown"));
    assert!(output.html.contains("No recorded actual departure"));
    assert!(
        output
            .html
            .contains("Reviewed &lt;script&gt;unsafe()&lt;/script&gt; &amp; counted")
    );
    assert!(!output.html.contains("<script>"));
    assert_eq!(output, trace_output(&value).unwrap());
}
