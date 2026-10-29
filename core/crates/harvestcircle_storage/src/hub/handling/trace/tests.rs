use super::*;
use crate::HubDispatchReceipt;
use crate::hub::stock::tests::{Fixture, command, id, now, operation, other, owner, query, setup};
use harvestcircle_domain::hub::departure::DepartureCommand;
use harvestcircle_domain::hub::dispatch::{
    DispatchAction, DispatchCommand, DispatchLineInput, DispatchSelection,
};
use harvestcircle_domain::hub::movement::{DiscrepancyAction, DiscrepancyCommand};
use harvestcircle_domain::hub::reference::{
    HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue, SourceAttribution,
    SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
};
use harvestcircle_domain::hub::stock::ReceiptIdentities;
use harvestcircle_domain::hub::trace::TraceScope;
use harvestcircle_domain::hub::treatment::{
    AcceptanceStatus, TreatmentAction, TreatmentCommand, TreatmentSelection,
};
use sqlx::Connection;

fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
fn history(trace: HubRecordId) -> HubTraceHistoryQuery {
    HubTraceHistoryQuery {
        trace,
        expected_revision: None,
        expected_catalog_revision: None,
        offset: 0,
        limit: 64,
    }
}
fn snapshot(trace: HubRecordId) -> HubTraceSnapshotQuery {
    HubTraceSnapshotQuery {
        trace,
        expected_revision: None,
        expected_catalog_revision: None,
    }
}
async fn receive(db: &Database, op: u32, source: SourceAttribution) -> crate::HubStockReceipt {
    let catalog = db.hub_references(owner()).await.unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let mut input = command(&catalog);
    input.expected_stock_revision = page.revision;
    input.source = source;
    db.apply_hub_receipt(owner(), operation(op), input, now())
        .await
        .unwrap()
}
fn source(op: u32, issuer: u8, certainty: SourceCertainty) -> SourceAttribution {
    let op = HubRecordId::from_hex(&operation(op).as_str().replace('-', "")).unwrap();
    SourceAttribution::Referenced {
        source: SourceReference {
            id: ReceiptIdentities::from_operation_bytes(op).unwrap().source,
            issuer: Some(id(issuer)),
            kind: SourceReferenceKind::SupplierReference,
            code: SourceCode::new("FX-1027").unwrap(),
            provenance: note("Manually reviewed original source label"),
        },
        certainty,
    }
}
async fn accept(db: &Database, group: HubRecordId, op: u32) {
    let current = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(op),
        TreatmentCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: TreatmentAction::SetAcceptance {
                selection: TreatmentSelection::new(vec![group]).unwrap(),
                status: AcceptanceStatus::Accepted,
            },
            occurred: Occurrence::Unknown,
            observation: note("Reviewed sample acceptance"),
        },
        now(),
    )
    .await
    .unwrap();
}
async fn issue(db: &Database, groups: &[HubRecordId], op: u32) -> HubDispatchReceipt {
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let mut lines = Vec::new();
    for id in groups {
        let current = db.hub_group(owner(), *id).await.unwrap();
        lines.push(DispatchLineInput {
            group: *id,
            expected_group_revision: current.group.revision,
            packs: current.group.packs,
        });
    }
    let prepared = db
        .apply_hub_dispatch(
            owner(),
            operation(op),
            DispatchCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                action: DispatchAction::Prepare {
                    recipient: id(3),
                    lines: DispatchSelection::new(lines).unwrap(),
                },
                observation: note("Reviewed all original population lines"),
            },
            now(),
        )
        .await
        .unwrap();
    db.apply_hub_dispatch(
        owner(),
        operation(op + 1),
        DispatchCommand {
            expected_stock_revision: prepared.record.updated.stock_revision,
            expected_catalog_revision: prepared.catalog_revision,
            action: DispatchAction::Issue {
                preparation: prepared.record.id,
                expected_revision: prepared.record.revision,
            },
            observation: note("As-issued sample identification"),
        },
        now(),
    )
    .await
    .unwrap()
}
async fn depart(db: &Database, paper: &HubDispatchReceipt, op: u32) -> crate::HubDepartureReceipt {
    let current = db
        .hub_dispatch_record(owner(), paper.record.id)
        .await
        .unwrap();
    db.apply_hub_departure(
        owner(),
        operation(op),
        DepartureCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            preparation: paper.record.id,
            expected_preparation_revision: paper.record.revision,
            identification: paper.record.versions.last().unwrap().id,
            occurred: Occurrence::Unknown,
            observation: note("All listed original packs actually left"),
        },
        now(),
    )
    .await
    .unwrap()
}
async fn counts(db: &Database) -> (i64, i64, i64, i64) {
    db.host().transaction(|tx|Box::pin(async move {
        sqlx::query_as("SELECT (SELECT count(*) FROM hub_stock_operations),(SELECT count(*) FROM hub_stock_events),(SELECT coalesce(sum(packs),0) FROM hub_stock_positions),(SELECT count(*) FROM durable_operations)")
            .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())
    })).await.unwrap()
}

#[tokio::test]
async fn broad_source_retrieval_keeps_all_unresolved_candidates_and_exact_colliding_issuers() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let a = receive(&db, 1, source(1, 5, SourceCertainty::Established)).await;
    let b = receive(&db, 2, source(2, 4, SourceCertainty::Established)).await;
    let possible = receive(&db, 3, source(3, 4, SourceCertainty::ReportedPossible)).await;
    let unknown = receive(&db, 4, SourceAttribution::Unknown).await;
    let SourceAttribution::Referenced { source, .. } = &a.original.trace.source else {
        panic!("known source")
    };
    let before = counts(&db).await;
    let page = db
        .hub_trace_page(
            owner(),
            HubTraceQuery {
                selection: TraceSelection {
                    scope: TraceScope::SourceImpact(source.id),
                    text: String::new(),
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    let ids = page
        .rows
        .iter()
        .map(|r| r.trace.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(ids.contains(&a.original.trace.id));
    assert!(ids.contains(&possible.original.trace.id));
    assert!(ids.contains(&unknown.original.trace.id));
    assert!(!ids.contains(&b.original.trace.id));
    let search = db
        .hub_trace_page(
            owner(),
            HubTraceQuery {
                selection: TraceSelection {
                    scope: TraceScope::All,
                    text: "FX-1027".into(),
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(search.total, 3);
    let issuer = db
        .hub_trace_page(
            owner(),
            HubTraceQuery {
                selection: TraceSelection {
                    scope: TraceScope::All,
                    text: "Fictional issuer".into(),
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(issuer.total, 1);
    assert_eq!(issuer.rows[0].trace, a.original.trace);
    assert_eq!(before, counts(&db).await);
    assert_eq!(
        db.hub_trace_page(other(), HubTraceQuery::default())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn fully_departed_populations_keep_all_material_line_history_originals_and_stable_reprints() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let a = receive(&db, 1, SourceAttribution::Unknown).await;
    let b = receive(&db, 2, SourceAttribution::Unknown).await;
    accept(&db, a.original.position.group, 10).await;
    accept(&db, b.original.position.group, 11).await;
    let paper = issue(
        &db,
        &[a.original.position.group, b.original.position.group],
        20,
    )
    .await;
    let version = paper.record.versions.last().unwrap();
    let original_output =
        harvestcircle_application::hub_output::identification_output(&paper.record, version.id)
            .unwrap();
    let secondary = version.material.lines.last().unwrap().trace.id;
    let primary = version.material.lines.first().unwrap().trace.id;
    assert_ne!(secondary, primary);
    let original = depart(&db, &paper, 30).await;
    let before = counts(&db).await;
    let page = db
        .hub_trace_page(owner(), HubTraceQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    assert!(
        page.rows
            .iter()
            .all(|r| r.balance.on_hand.value() == 0 && r.balance.departed.value() == 20)
    );
    let current = db
        .hub_trace_snapshot(owner(), snapshot(secondary))
        .await
        .unwrap();
    assert!(current.on_hand.is_empty());
    assert_eq!(current.exits.len(), 1);
    assert_eq!(
        current.exits[0].departure.recipient,
        version.material.recipient
    );
    assert_eq!(current.exits[0].packs.value(), 20);
    let events = db
        .hub_trace_history(owner(), history(secondary))
        .await
        .unwrap();
    assert_eq!(events.total, 5);
    assert_eq!(
        events
            .rows
            .iter()
            .filter(|r| matches!(r.kind, HubTraceHistoryKind::Dispatch(_)))
            .count(),
        2
    );
    assert_eq!(events.rows.last().unwrap().operation, operation(30));
    assert!(
        events
            .rows
            .windows(2)
            .all(|w| w[0].revision.value() < w[1].revision.value())
    );
    assert!(
        events
            .rows
            .iter()
            .filter(|r| matches!(r.kind, HubTraceHistoryKind::Dispatch(_)))
            .all(|r| r.occurred.is_none())
    );
    assert_eq!(before, counts(&db).await);
    let catalog = db.hub_references(owner()).await.unwrap();
    db.apply_hub_reference(
        owner(),
        operation(40),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(3),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Party {
                    label: HubLabel::new("Renamed current recipient").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    let reloaded = db
        .hub_dispatch_record(owner(), paper.record.id)
        .await
        .unwrap();
    assert_eq!(
        original_output,
        harvestcircle_application::hub_output::identification_output(&reloaded.record, version.id)
            .unwrap()
    );
    assert_eq!(
        db.hub_trace_snapshot(other(), snapshot(secondary))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.hub_trace_history(other(), history(secondary))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let stored = db
        .hub_trace_snapshot(owner(), snapshot(secondary))
        .await
        .unwrap();
    db.close().await.unwrap();
    let reopened = fixture.open().await;
    assert_eq!(
        stored,
        reopened
            .hub_trace_snapshot(owner(), snapshot(secondary))
            .await
            .unwrap()
    );
    assert_eq!(
        original,
        reopened
            .hub_departure_operation(owner(), operation(30))
            .await
            .unwrap()
            .unwrap()
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn replacement_history_remains_linked_to_removed_material_without_claiming_later_unused_scope()
 {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let a = receive(&db, 1, SourceAttribution::Unknown).await;
    let b = receive(&db, 2, SourceAttribution::Unknown).await;
    let paper = issue(
        &db,
        &[a.original.position.group, b.original.position.group],
        20,
    )
    .await;
    let version = paper.record.versions.last().unwrap();
    let original_output =
        harvestcircle_application::hub_output::identification_output(&paper.record, version.id)
            .unwrap();
    let current = db
        .hub_group(owner(), a.original.position.group)
        .await
        .unwrap();
    let replaced = db
        .apply_hub_dispatch(
            owner(),
            operation(30),
            DispatchCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                action: DispatchAction::Replace {
                    preparation: paper.record.id,
                    expected_revision: paper.record.revision,
                    recipient: id(4),
                    lines: DispatchSelection::new(vec![DispatchLineInput {
                        group: current.group.id,
                        expected_group_revision: current.group.revision,
                        packs: current.group.packs,
                    }])
                    .unwrap(),
                },
                observation: note("Replace both-source identification with only the first source"),
            },
            now(),
        )
        .await
        .unwrap();
    db.apply_hub_dispatch(
        owner(),
        operation(31),
        DispatchCommand {
            expected_stock_revision: replaced.record.updated.stock_revision,
            expected_catalog_revision: replaced.catalog_revision,
            action: DispatchAction::MarkUnused {
                preparation: paper.record.id,
                expected_revision: replaced.record.revision,
            },
            observation: note("Only the replacement identification was unused"),
        },
        now(),
    )
    .await
    .unwrap();
    let old = db
        .hub_trace_history(owner(), history(b.original.trace.id))
        .await
        .unwrap();
    assert!(old.rows.iter().any(|r| r.operation == operation(30)));
    assert!(!old.rows.iter().any(|r| r.operation == operation(31)));
    let kept = db
        .hub_trace_history(owner(), history(a.original.trace.id))
        .await
        .unwrap();
    assert!(kept.rows.iter().any(|r| r.operation == operation(30)));
    assert!(kept.rows.iter().any(|r| r.operation == operation(31)));
    let record = db
        .hub_dispatch_record(owner(), paper.record.id)
        .await
        .unwrap();
    assert_eq!(
        original_output,
        harvestcircle_application::hub_output::identification_output(&record.record, version.id)
            .unwrap()
    );
    assert!(
        db.hub_trace_snapshot(owner(), snapshot(b.original.trace.id))
            .await
            .unwrap()
            .exits
            .is_empty()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn bounded_history_continuations_pin_both_versions_and_reads_have_no_effect() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let intake = receive(&db, 1, SourceAttribution::Unknown).await;
    for op in 10..80 {
        let current = db
            .hub_group(owner(), intake.original.position.group)
            .await
            .unwrap();
        db.apply_hub_discrepancy(
            owner(),
            operation(op),
            DiscrepancyCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                action: DiscrepancyAction::Open {
                    trace: intake.original.trace.id,
                    group: Some(intake.original.position.group),
                    observation: note("Bounded manual observation, no quantity change"),
                },
            },
            now(),
        )
        .await
        .unwrap();
    }
    let before = counts(&db).await;
    let first = db
        .hub_trace_history(owner(), history(intake.original.trace.id))
        .await
        .unwrap();
    assert_eq!(first.total, 71);
    assert_eq!(first.rows.len(), 64);
    assert!(first.more);
    let next = HubTraceHistoryQuery {
        expected_revision: Some(first.revision),
        expected_catalog_revision: Some(first.catalog_revision),
        offset: 64,
        ..history(first.trace)
    };
    let last = db.hub_trace_history(owner(), next.clone()).await.unwrap();
    assert_eq!(last.rows.len(), 7);
    assert!(!last.more);
    assert!(
        first.rows.last().unwrap().revision.value() < last.rows.first().unwrap().revision.value()
    );
    assert_eq!(before, counts(&db).await);
    let mut unsafe_next = next.clone();
    unsafe_next.expected_catalog_revision = None;
    assert_eq!(
        db.hub_trace_history(owner(), unsafe_next)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    let catalog = db.hub_references(owner()).await.unwrap();
    db.apply_hub_reference(
        owner(),
        operation(90),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(1),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Product {
                    label: HubLabel::new("Current renamed product").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_trace_history(owner(), next)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let old = HubTraceSnapshotQuery {
        expected_revision: Some(first.revision),
        expected_catalog_revision: Some(first.catalog_revision),
        ..snapshot(first.trace)
    };
    assert_eq!(
        db.hub_trace_snapshot(owner(), old)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn trace_snapshot_and_history_never_mask_damaged_departure_as_missing_or_unavailable() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let intake = receive(&db, 1, SourceAttribution::Unknown).await;
    accept(&db, intake.original.position.group, 10).await;
    let paper = issue(&db, &[intake.original.position.group], 20).await;
    depart(&db, &paper, 30).await;
    let mut raw = fixture.raw().await;
    sqlx::query("DROP TRIGGER hub_departure_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_departures SET result_hash=zeroblob(32)")
        .execute(&mut raw)
        .await
        .unwrap();
    let guard = super::super::departure::schema::MIGRATION_SQL
        .split(";\n")
        .find(|sql| sql.starts_with("CREATE TRIGGER hub_departure_no_update "))
        .unwrap();
    sqlx::query(guard).execute(&mut raw).await.unwrap();
    raw.close().await.unwrap();
    assert_eq!(
        db.hub_trace_snapshot(owner(), snapshot(intake.original.trace.id))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_trace_history(owner(), history(intake.original.trace.id))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_trace_page(owner(), HubTraceQuery::default())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    db.close().await.unwrap();
}
