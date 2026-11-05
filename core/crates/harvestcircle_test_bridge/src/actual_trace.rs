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
    HubTraceSnapshotDto::try_from(actual_trace_snapshot()).unwrap()
}

/// Encoding-only whole-event recipient history. No runtime, database or grant.
#[uniffi::export]
pub fn recipient_trace_encoding_probe() -> HubTraceSnapshotDto {
    HubTraceSnapshotDto::try_from(recipient_trace_snapshot()).unwrap()
}

fn recipient_trace_snapshot() -> TraceSnapshot {
    use harvestcircle_domain::hub::corrections::*;
    let mut snapshot = actual_trace_snapshot();
    let actual = &snapshot.actual_departures[0];
    let original = OriginalRecipientAttribution {
        hub: snapshot.hub,
        family: CorrectedDepartureFamily::ReviewedActual,
        operation: actual.operation,
        stock_revision: actual.stock_revision,
        catalog_revision: actual.comparison.facts.expected_catalog_revision,
        recorded_at: actual.recorded_at,
        actor: actual.actor,
        recipient: actual.comparison.scope.recipient.clone(),
        populations: vec![snapshot.trace.id],
        issued_versions: Vec::new(),
    };
    let catalog = ReferenceCatalog::restore(
        snapshot.catalog_revision,
        vec![
            snapshot.trace.product.clone(),
            snapshot.trace.variant.clone(),
            snapshot.trace.initial_location.clone(),
            original.recipient.clone(),
        ],
    )
    .unwrap();
    let command = RecipientAttributionCommand {
        departure_operation: original.operation,
        expected_stock_revision: snapshot.stock_revision,
        expected_catalog_revision: snapshot.catalog_revision,
        expected_prior: None,
        recipient: None,
        basis: note("Encoding-only explicit unknown recipient and retained original/manual impact"),
    };
    let plan = command
        .plan(
            &RecipientAttributionContext {
                cut: AttributionCut {
                    hub: snapshot.hub,
                    stock_revision: snapshot.stock_revision,
                    catalog: &catalog,
                    latest_recorded_at: at(190),
                    uncertain_submissions: false,
                },
                original: &original,
                history: &[],
                linked_issued_versions: &[],
            },
            op(84),
            reviewer(),
            at(210),
        )
        .unwrap();
    snapshot.stock_revision = plan.stamp.stock_revision;
    snapshot.recipient_corrections = vec![plan];
    snapshot
}

/// Domain-checked encoding only: zero effective quantity and Unknown recipient.
/// No database, native submission, physical credit or execution grant is created.
#[uniffi::export]
pub fn actual_link_interpretation_encoding_probe() -> harvestcircle_ffi::HubActualDuplicateDto {
    use harvestcircle_domain::hub::corrections::{
        DepartureQuantityAccount, DepartureQuantityAmendment, DepartureQuantityDelta,
        OriginalDepartureQuantity,
    };
    let snapshot = recipient_trace_snapshot();
    let actual = &snapshot.actual_departures[0];
    let known = actual.physical_projection().unwrap();
    let line = &known.lines[0];
    let quantity = DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            operation: known.operation,
            group: line.group,
            trace: known.trace,
            variant: known.variant.id,
            stock_revision: known.stock_revision,
            packs: line.packs,
        },
        vec![DepartureQuantityAmendment {
            operation: op(86),
            stock_revision: revision(snapshot.stock_revision.value() + 1),
            before: line.packs,
            after: packs(0),
            delta: DepartureQuantityDelta::IdentifiedFoundRemainder(line.packs),
        }],
        packs(0),
    )
    .unwrap();
    let interpretation = ActualKnownInterpretation {
        stock_revision: revision(snapshot.stock_revision.value() + 1),
        catalog_revision: snapshot.catalog_revision,
        latest_recorded_at: at(220),
        original: known.clone(),
        recipient_original: snapshot.recipient_corrections[0].original.clone(),
        recipient_history: snapshot.recipient_corrections,
        quantities: vec![quantity],
    };
    interpretation.view().unwrap();
    let command = ActualDuplicateLink {
        report: actual.report.id,
        known_operation: known.operation,
        known_trace: known.trace,
        known_variant: known.variant.id,
        verification: note(
            "Encoding-only explicit original, zero quantity and Unknown association",
        ),
    };
    let plan = command
        .plan_with_interpretation(
            &actual.report,
            &interpretation,
            None,
            op(90),
            reviewer(),
            at(230),
        )
        .unwrap();
    harvestcircle_ffi::HubActualDuplicateDto::try_from(&plan).unwrap()
}

/// Encoding-only source interpretation. No database, submission or native grant.
#[uniffi::export]
pub fn source_trace_encoding_probe() -> HubTraceSnapshotDto {
    use harvestcircle_domain::hub::corrections::*;
    let mut snapshot = actual_trace_snapshot();
    let catalog = ReferenceCatalog::restore(
        snapshot.catalog_revision,
        vec![
            snapshot.trace.product.clone(),
            snapshot.trace.variant.clone(),
            snapshot.trace.initial_location.clone(),
        ],
    )
    .unwrap();
    let original = OriginalSourceAttribution {
        hub: snapshot.hub,
        operation: op(1),
        trace: snapshot.trace.id,
        stock_revision: revision(2),
        catalog_revision: snapshot.catalog_revision,
        recorded_at: snapshot.trace.recorded_at,
        actor: snapshot.trace.actor,
        source: AmendedSource {
            source: snapshot.trace.source.clone(),
            issuer: snapshot.trace.source_issuer.clone(),
        },
    };
    let actual = &snapshot.actual_departures[0];
    let affected = vec![AttributionAffectedDeparture {
        operation: actual.operation,
        populations: vec![snapshot.trace.id],
        recipients: vec![AmendedRecipient::Known(
            actual.comparison.scope.recipient.clone(),
        )],
        issued_versions: Vec::new(),
    }];
    let command = SourceAttributionCommand {
        trace: snapshot.trace.id,
        expected_stock_revision: snapshot.stock_revision,
        expected_catalog_revision: snapshot.catalog_revision,
        expected_prior: None,
        source: SourceAttribution::Referenced {
            source: SourceReference {
                id: id(75),
                issuer: None,
                kind: SourceReferenceKind::Other,
                code: SourceCode::new("ENCODING-ONLY-POSSIBLE-SOURCE").unwrap(),
                provenance: note(
                    "Encoding fixture retains possible origin, not a native source grant",
                ),
            },
            certainty: SourceCertainty::ReportedPossible,
        },
        basis: note("Encoding-only original/current source and downstream scope"),
    };
    let plan = command
        .plan(
            &SourceAttributionContext {
                cut: AttributionCut {
                    hub: snapshot.hub,
                    stock_revision: snapshot.stock_revision,
                    catalog: &catalog,
                    latest_recorded_at: at(190),
                    uncertain_submissions: false,
                },
                original: &original,
                history: &[],
                known_sources: &[],
                affected: &affected,
                issued_versions: &[],
            },
            op(83),
            reviewer(),
            at(210),
        )
        .unwrap();
    snapshot.stock_revision = plan.stamp.stock_revision;
    snapshot.source_corrections = vec![plan];
    HubTraceSnapshotDto::try_from(snapshot).unwrap()
}

/// Domain-checked encoding only, never a persisted correction or admission.
#[uniffi::export]
pub fn corrected_trace_encoding_probe() -> HubTraceSnapshotDto {
    use harvestcircle_domain::hub::corrections::{
        DepartureQuantityAccount, DepartureQuantityAmendment, DepartureQuantityDelta,
        OriginalDepartureQuantity,
    };
    let mut snapshot = actual_trace_snapshot();
    let actual = &snapshot.actual_departures[0];
    let physical = actual.physical_projection().unwrap();
    let line = &physical.lines[0];
    let quantity = DepartureQuantityAccount::restore(
        OriginalDepartureQuantity {
            operation: actual.operation,
            group: line.group,
            trace: snapshot.trace.id,
            variant: snapshot.trace.variant.id,
            stock_revision: actual.stock_revision,
            packs: line.packs,
        },
        vec![DepartureQuantityAmendment {
            operation: op(82),
            stock_revision: revision(snapshot.stock_revision.value() + 1),
            before: packs(2),
            after: packs(1),
            delta: DepartureQuantityDelta::IdentifiedFoundRemainder(packs(1)),
        }],
        packs(0),
    )
    .unwrap();
    snapshot.stock_revision = revision(snapshot.stock_revision.value() + 1);
    let mut child = snapshot.on_hand[0].clone();
    child.group = id(90);
    child.revision = revision(1);
    child.packs = packs(1);
    child.treatment.group = child.group;
    child.treatment.packs = child.packs;
    child.treatment.acceptance =
        harvestcircle_domain::hub::treatment::AcceptanceStatus::ReviewRequired;
    child.treatment.acceptance_decision = None;
    child.identification = Some(
        harvestcircle_domain::hub::movement::PhysicalIdentification {
            label: HubLabel::new("Independent encoding-only found child").unwrap(),
            observation: note("Present identity without restored work"),
        },
    );
    snapshot.on_hand.push(child);
    snapshot.quantity_corrections = vec![quantity];
    HubTraceSnapshotDto::try_from(snapshot).unwrap()
}

fn actual_trace_snapshot() -> TraceSnapshot {
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
    TraceSnapshot {
        source_corrections: Vec::new(),
        recipient_corrections: Vec::new(),
        quantity_corrections: vec![],
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
    }
}

/// Native enum/record encoding only; these are not a persisted history or grant.
#[uniffi::export]
pub fn actual_trace_history_encoding_probe() -> HubTraceHistoryPageDto {
    let kinds = [
        HubTraceHistoryKindDto::ActualReported,
        HubTraceHistoryKindDto::ActualReconciled,
        HubTraceHistoryKindDto::ActualLinked,
        HubTraceHistoryKindDto::FoundCorrected,
        HubTraceHistoryKindDto::AdditionalCorrected,
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
        stock_revision: 8,
        catalog_revision: 8,
        offset: 0,
        total: 5,
        more: false,
        rows,
    }
}

/// Encoding-only page roles; no database, admission or persisted closure claim.
#[uniffi::export]
pub fn actual_report_resolution_encoding_probe() -> harvestcircle_ffi::HubActualReportPageDto {
    use harvestcircle_ffi::{
        HubActualReportPageDto, HubActualReportResolutionKindDto, HubActualReportRowDto,
    };
    let report = actual_trace_snapshot().actual_departures.remove(0).report;
    let rows = [
        None,
        Some(HubActualReportResolutionKindDto::ActualReview),
        Some(HubActualReportResolutionKindDto::AdditionalQuantity),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, kind)| HubActualReportRowDto {
        report: (&report).into(),
        review_operation_id: kind
            .map(|_| format!("01900000-0000-7000-8000-0000000000{:02x}", 90 + n)),
        resolution_kind: kind,
    })
    .collect();
    HubActualReportPageDto {
        stock_revision: 8,
        catalog_revision: 8,
        offset: 0,
        total: 3,
        more: false,
        rows,
    }
}
