//! Encoding-only read fixture built by the closed domain policy. It opens no
//! database/runtime and grants no authority or claimed persisted stock effect.
use harvestcircle_domain::{
    PublicKey, UnixTimestamp,
    hub::{
        HubId, HubRevision,
        actuals::*,
        movement::{PhysicalStockGroup, WorkAllocation, WorkProgress, WorkRequirement},
        portions::{PhysicalLineage, PhysicalLineageNode},
        reference::*,
        stock::{Occurrence, PopulationOrigin, QuantityWork, StockPopulation},
        trace::{TracePosition, TraceSnapshot},
        treatment::TreatmentState,
    },
};
use harvestcircle_ffi::{
    HubTraceHistoryEntryDto, HubTraceHistoryKindDto, HubTraceHistoryPageDto, HubTraceSnapshotDto,
};

fn id(n: u8) -> HubRecordId {
    HubRecordId::from_bytes([n; 16]).unwrap()
}
fn op(n: u8) -> HubRecordId {
    HubRecordId::from_hex(&format!("019000000000700080000000000000{n:02x}")).unwrap()
}
fn revision(n: u64) -> HubRevision {
    HubRevision::from_value(n).unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn note(text: &str) -> SourceProvenance {
    SourceProvenance::new(text).unwrap()
}
fn reporter() -> PublicKey {
    PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap()
}
fn reviewer() -> PublicKey {
    PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5").unwrap()
}

#[uniffi::export]
pub fn actual_trace_encoding_probe() -> HubTraceSnapshotDto {
    let label = |s| HubLabel::new(s).unwrap();
    let reference = |n, value| ReferenceRecord {
        id: id(n),
        revision: revision(1),
        value,
    };
    let historical = reference(
        5,
        ReferenceValue::Location {
            label: label("Historical A1"),
            purpose: LocationPurpose::Receiving,
        },
    );
    let mut renamed = historical.clone();
    renamed.revision = revision(2);
    renamed.value = ReferenceValue::Location {
        label: label("Renamed A2"),
        purpose: LocationPurpose::Receiving,
    };
    let current = ReferenceRecord {
        id: id(6),
        revision: revision(2),
        value: ReferenceValue::Location {
            label: label("Current B2"),
            purpose: LocationPurpose::Storage,
        },
    };
    let planned = reference(
        7,
        ReferenceValue::Location {
            label: label("Planned C1"),
            purpose: LocationPurpose::Storage,
        },
    );
    let catalog = ReferenceCatalog::restore(
        revision(8),
        vec![
            reference(
                1,
                ReferenceValue::Product {
                    label: label("Sample product"),
                },
            ),
            reference(
                2,
                ReferenceValue::Pack {
                    product: id(1),
                    label: label("Sample whole pack"),
                    nominal_grams: None,
                },
            ),
            reference(
                3,
                ReferenceValue::Party {
                    label: label("Verified recipient"),
                },
            ),
            renamed,
            current.clone(),
            planned.clone(),
        ],
    )
    .unwrap();
    let group = PhysicalStockGroup {
        id: id(20),
        trace: id(21),
        variant: id(2),
        parent: None,
        identification: None,
        location: current.id,
        packs: packs(20),
        revision: revision(2),
        causal_floor: Some(at(155)),
        work: vec![WorkAllocation {
            requirement: WorkRequirement {
                id: id(22),
                trace: id(21),
                variant: id(2),
                target: Some(planned.clone()),
                quantity: QuantityWork::pending(packs(20)),
                revision: revision(1),
            },
            progress: WorkProgress::Pending,
        }],
    };
    let hub = HubId::from_bytes([1; 16]).unwrap();
    let report = ActualReportCommand {
        variant: Some(id(2)),
        recipient: Some(id(3)),
        packs: Some(packs(2)),
        sources: ActualReportSources::Unknown,
        occurred: Occurrence::Interval {
            earliest: at(140),
            latest: at(160),
        },
        observation: note("Original crossing-clock claim remains unchanged"),
    }
    .preserve(
        hub,
        revision(3),
        catalog.revision(),
        op(80),
        reporter(),
        at(150),
    )
    .unwrap();
    let graph = PhysicalLineage::new(
        hub,
        group.trace,
        group.variant,
        report.stock_revision,
        vec![PhysicalLineageNode {
            recovery: None,
            id: group.id,
            parent: None,
            trace: group.trace,
            variant: group.variant,
            created_revision: revision(2),
            packs: group.packs,
        }],
    )
    .unwrap();
    let source = HistoricalActualSource {
        operation: op(1),
        actor: reporter(),
        hub,
        stock_revision: revision(2),
        catalog_revision: revision(7),
        group: HistoricalActualPosition {
            id: group.id,
            trace: group.trace,
            variant: group.variant,
            parent: None,
            location: historical.id,
            packs: packs(20),
            revision: revision(1),
        },
        location: historical.clone(),
        occurred: Occurrence::Known(at(100)),
        recorded_at: at(100),
    };
    let activity = [ActualPhysicalActivity {
        sequence: 2,
        operation: op(2),
        actor: reporter(),
        stock_revision: revision(3),
        hub,
        trace: group.trace,
        group: group.id,
        location: Some(current.id),
        kind: ActualPhysicalActivityKind::LocationChanged,
        occurred: Occurrence::Known(at(155)),
        recorded_at: at(170),
    }];
    let treatment = TreatmentState::default();
    let command = ActualNewDepartureCommand {
        facts: ActualReviewFacts {
            report: report.id,
            expected_stock_revision: graph.revision(),
            expected_catalog_revision: catalog.revision(),
            group: group.id,
            expected_group_revision: group.revision,
            packs: packs(2),
            recipient: id(3),
            occurred: Occurrence::Known(at(145)),
            source_evidence: note("Historical cohort A verified"),
            quantity_evidence: note("Two whole packs verified"),
            recipient_evidence: note("Recipient independently verified"),
            timing_evidence: note("145 independently verified within original140..160"),
        },
        source_operation: source.operation,
        source_group: group.id,
        duplicate_review_basis: note("Original departures compared without a duplicate-clear flag"),
    };
    let context = ActualNewDepartureContext {
        review: ActualReviewContext {
            hub,
            stock_revision: graph.revision(),
            catalog: &catalog,
            original: &report,
            group: &group,
            lineage: &graph,
            treatment: &treatment,
            uncertain_submissions: false,
        },
        historical_source: ActualSourceContext {
            source: &source,
            lineage: &graph,
            balancing_group: &group,
            subsequent_activity: &activity,
        },
        known_departures: &[],
        prior_review: None,
    };
    let plan = command.plan(&context, op(81), reviewer(), at(190)).unwrap();
    let mut after = graph.nodes().to_vec();
    after[0].packs = plan.reduction.on_hand_after;
    let after =
        PhysicalLineage::new(hub, group.trace, group.variant, plan.stock_revision, after).unwrap();
    let projection = treatment.project(&after, catalog.handling_basis()).unwrap();
    let snapshot = TraceSnapshot {
        hub,
        stock_revision: plan.stock_revision,
        catalog_revision: catalog.revision(),
        trace: StockPopulation {
            id: group.trace,
            variant: catalog.get(id(2)).unwrap().clone(),
            product: catalog.get(id(1)).unwrap().clone(),
            supplier: None,
            producer: None,
            source: SourceAttribution::Unknown,
            source_issuer: None,
            original_packs: packs(20),
            initial_location: historical,
            origin: PopulationOrigin::Receipt,
            planned_location: Some(planned),
            occurred: Occurrence::Known(at(100)),
            recorded_at: at(100),
            actor: reporter(),
            observation: note("Original twenty sample packs"),
        },
        on_hand: vec![TracePosition {
            group: group.id,
            revision: plan.reduction.after_revision,
            packs: plan.reduction.on_hand_after,
            location: current,
            identification: None,
            treatment: projection
                .groups
                .into_iter()
                .find(|g| g.group == group.id)
                .unwrap(),
        }],
        exits: vec![],
        count_losses: vec![],
        dispositions: vec![],
        actual_departures: vec![plan],
    };
    HubTraceSnapshotDto::try_from(snapshot).unwrap()
}

/// Native enum/record encoding only; these are not a persisted history or grant.
#[uniffi::export]
pub fn actual_trace_history_encoding_probe() -> HubTraceHistoryPageDto {
    let kinds = [
        HubTraceHistoryKindDto::ActualReported,
        HubTraceHistoryKindDto::ActualReconciled,
        HubTraceHistoryKindDto::ActualLinked,
    ];
    let rows = kinds
        .into_iter()
        .enumerate()
        .map(|(n, kind)| HubTraceHistoryEntryDto {
            operation_id: format!("01900000-0000-7000-8000-0000000000{:02x}", 80 + n),
            stock_revision: 4 + n as u64,
            kind,
            actor_public_key: if n == 0 { reporter() } else { reviewer() }.to_hex(),
            recorded_at_unix_s: 150 + 20 * n as i64,
            occurrence: Some(Occurrence::Known(at(145)).into()),
            observation: "Encoding-only history role fixture".to_owned(),
            result_id: id(23).to_hex(),
        })
        .collect();
    HubTraceHistoryPageDto {
        trace_id: id(21).to_hex(),
        stock_revision: 6,
        catalog_revision: 8,
        offset: 0,
        total: 3,
        more: false,
        rows,
    }
}
