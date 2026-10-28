use super::*;
use crate::hub::stock::tests::{
    Fixture, command as receipt_command, id, now, operation, other, owner, query as stock_query,
    setup,
};
use crate::{Database, HubStockReceipt};
use harvestcircle_domain::hub::dispatch::{
    DispatchAction, DispatchLineInput, DispatchSelection, IdentificationStatus,
};
use harvestcircle_domain::hub::reference::{
    ReferenceCommand, ReferenceMutation, ReferenceValue, SourceProvenance,
};
use sqlx::{Connection, Row};

fn query() -> HubDispatchQuery {
    HubDispatchQuery {
        expected_revision: None,
        expected_catalog_revision: None,
        offset: 0,
        limit: 64,
        filter: HubDispatchFilter::All,
        text: String::new(),
    }
}
async fn open(db: &Database) -> HubStockReceipt {
    setup(db).await;
    for (number, label) in [(8, "Recipient A"), (9, "Recipient B")] {
        let catalog = db.hub_references(owner()).await.unwrap();
        db.apply_hub_reference(
            owner(),
            operation(1000 + u32::from(number)),
            ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: ReferenceMutation::Create {
                    id: id(number),
                    value: ReferenceValue::Party {
                        label: HubLabel::new(label).unwrap(),
                    },
                },
            },
            now(),
        )
        .await
        .unwrap();
    }
    let catalog = db.hub_references(owner()).await.unwrap();
    db.apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap()
}
async fn selection(db: &Database, group: HubRecordId) -> DispatchSelection {
    let current = db.hub_group(owner(), group).await.unwrap();
    DispatchSelection::new(vec![DispatchLineInput {
        group,
        expected_group_revision: current.group.revision,
        packs: current.group.packs,
    }])
    .unwrap()
}
async fn command(db: &Database, action: DispatchAction) -> DispatchCommand {
    let current = db.hub_dispatch_page(owner(), query()).await.unwrap();
    DispatchCommand {
        expected_stock_revision: current.revision,
        expected_catalog_revision: current.catalog_revision,
        action,
        observation: SourceProvenance::new("Manually reviewed fictional document basis").unwrap(),
    }
}
async fn prepare(db: &Database, stock: &HubStockReceipt, n: u32) -> HubDispatchReceipt {
    let input = command(
        db,
        DispatchAction::Prepare {
            recipient: id(8),
            lines: selection(db, stock.original.position.group).await,
        },
    )
    .await;
    db.apply_hub_dispatch(owner(), operation(n), input, now())
        .await
        .unwrap()
}
async fn issue(db: &Database, record: &DispatchRecord, n: u32) -> HubDispatchReceipt {
    let input = command(
        db,
        DispatchAction::Issue {
            preparation: record.id,
            expected_revision: record.revision,
        },
    )
    .await;
    db.apply_hub_dispatch(owner(), operation(n), input, now())
        .await
        .unwrap()
}

#[tokio::test]
async fn native_documents_are_nonreserving_and_retain_issued_versions_originals_and_restart() {
    let f = Fixture::new();
    let db = f.open().await;
    let stock = open(&db).await;
    let positions = db
        .hub_stock_page(owner(), stock_query())
        .await
        .unwrap()
        .rows;
    let work = db
        .hub_work_page(owner(), HubHandlingQuery::default())
        .await
        .unwrap()
        .rows;
    let prepared = prepare(&db, &stock, 20).await;
    let issued = issue(&db, &prepared.record, 21).await;
    let first = issued.record.versions[0].clone();
    let replace = command(
        &db,
        DispatchAction::Replace {
            preparation: issued.record.id,
            expected_revision: issued.record.revision,
            recipient: id(9),
            lines: selection(&db, stock.original.position.group).await,
        },
    )
    .await;
    let replaced = db
        .apply_hub_dispatch(owner(), operation(22), replace, now())
        .await
        .unwrap();
    assert_eq!(replaced.record.versions[0].material, first.material);
    assert_eq!(replaced.record.versions[0].issued, first.issued);
    assert_eq!(
        replaced.record.versions[0].status,
        IdentificationStatus::Superseded
    );
    assert_eq!(replaced.record.versions[1].previous, Some(first.id));
    assert_eq!(replaced.record.versions[1].material.recipient.id, id(9));
    let unused = command(
        &db,
        DispatchAction::MarkUnused {
            preparation: replaced.record.id,
            expected_revision: replaced.record.revision,
        },
    )
    .await;
    let ended = db
        .apply_hub_dispatch(owner(), operation(23), unused.clone(), now())
        .await
        .unwrap();
    assert_eq!(
        ended.record.versions[1].status,
        IdentificationStatus::Unused
    );
    assert_eq!(
        ended,
        db.apply_hub_dispatch(owner(), operation(23), unused, now())
            .await
            .unwrap()
    );
    let catalog = db.hub_references(owner()).await.unwrap();
    db.apply_hub_reference(
        owner(),
        operation(1010),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(9),
                expected_record_revision: HubRevision::INITIAL,
                value: ReferenceValue::Party {
                    label: HubLabel::new("Renamed current recipient B").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        ended.record.versions[1]
            .material
            .recipient
            .value
            .label()
            .as_str(),
        "Recipient B"
    );
    assert_eq!(
        db.hub_dispatch_record(owner(), ended.record.id)
            .await
            .unwrap()
            .record,
        ended.record
    );
    assert_eq!(
        db.hub_dispatch_operation(owner(), issued.operation.clone())
            .await
            .unwrap(),
        Some(issued.clone())
    );
    assert_eq!(
        db.hub_dispatch_operation(owner(), prepared.operation.clone())
            .await
            .unwrap(),
        Some(prepared.clone())
    );
    assert_eq!(
        positions,
        db.hub_stock_page(owner(), stock_query())
            .await
            .unwrap()
            .rows
    );
    assert_eq!(
        work,
        db.hub_work_page(owner(), HubHandlingQuery::default())
            .await
            .unwrap()
            .rows
    );
    let page = db
        .hub_dispatch_page(
            owner(),
            HubDispatchQuery {
                filter: HubDispatchFilter::Unused,
                ..query()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].state, HubDispatchState::Unused);
    assert_eq!(page.rows[0].whole_packs, 20);
    db.close().await.unwrap();
    let db = f.open().await;
    assert_eq!(
        db.hub_dispatch_record(owner(), ended.record.id)
            .await
            .unwrap()
            .record,
        ended.record
    );
    assert_eq!(
        db.hub_dispatch_operation(owner(), issued.operation.clone())
            .await
            .unwrap(),
        Some(issued)
    );
    assert_eq!(
        stock,
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap()
            .unwrap()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_owner_replay_stale_competitors_and_pinned_queries_preserve_one_effect() {
    let f = Fixture::new();
    let db = f.open().await;
    let stock = open(&db).await;
    let input = command(
        &db,
        DispatchAction::Prepare {
            recipient: id(8),
            lines: selection(&db, stock.original.position.group).await,
        },
    )
    .await;
    assert_eq!(
        db.apply_hub_dispatch(other(), operation(20), input.clone(), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let (a, b) = tokio::join!(
        db.apply_hub_dispatch(owner(), operation(20), input.clone(), now()),
        db.apply_hub_dispatch(owner(), operation(21), input.clone(), now())
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let winner = a.or(b).unwrap();
    assert_eq!(
        winner,
        db.apply_hub_dispatch(owner(), winner.operation.clone(), input.clone(), now())
            .await
            .unwrap()
    );
    let mut changed = input;
    changed.observation = SourceProvenance::new("Changed operation intent").unwrap();
    assert_eq!(
        db.apply_hub_dispatch(owner(), winner.operation.clone(), changed, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let first = db
        .hub_dispatch_page(
            owner(),
            HubDispatchQuery {
                limit: 1,
                ..query()
            },
        )
        .await
        .unwrap();
    assert_eq!(first.total, 1);
    assert_eq!(
        db.hub_dispatch_record(other(), winner.record.id)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let _other = prepare(&db, &stock, 22).await;
    let stale = HubDispatchQuery {
        expected_revision: Some(first.revision),
        expected_catalog_revision: Some(first.catalog_revision),
        offset: 1,
        ..query()
    };
    assert_eq!(
        db.hub_dispatch_page(owner(), stale)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_dispatch_page(owner(), query()).await.unwrap().total,
        2
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_failure_after_document_inserts_rolls_back_every_effect() {
    let f = Fixture::new();
    let db = f.open().await;
    let stock = open(&db).await;
    let before = db.hub_stock_page(owner(), stock_query()).await.unwrap();
    let input = command(
        &db,
        DispatchAction::Prepare {
            recipient: id(8),
            lines: selection(&db, stock.original.position.group).await,
        },
    )
    .await;
    let mut raw = f.raw().await;
    let attempt = input.clone();
    let result = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let recorded =
                    super::write::apply_on(tx, owner(), operation(20), attempt, now()).await?;
                assert_eq!(
                    recorded
                        .record
                        .draft
                        .as_ref()
                        .unwrap()
                        .total_packs()
                        .unwrap(),
                    20
                );
                Err::<(), SafeError>(crate::db::storage_unavailable())
            })
        })
        .await;
    assert!(result.is_err());
    let counts=sqlx::query("SELECT (SELECT count(*) FROM hub_dispatch_materials) AS materials,(SELECT count(*) FROM hub_dispatch_material_lines) AS lines,(SELECT count(*) FROM hub_dispatch_changes) AS changes,(SELECT count(*) FROM hub_dispatch_identifications) AS issued").fetch_one(&mut raw).await.unwrap();
    for field in ["materials", "lines", "changes", "issued"] {
        assert_eq!(counts.get::<i64, _>(field), 0);
    }
    assert_eq!(
        before,
        db.hub_stock_page(owner(), stock_query()).await.unwrap()
    );
    assert_eq!(
        db.hub_dispatch_operation(owner(), operation(20))
            .await
            .unwrap(),
        None
    );
    let recorded = db
        .apply_hub_dispatch(owner(), operation(20), input, now())
        .await
        .unwrap();
    assert_eq!(recorded.record.draft.unwrap().total_packs().unwrap(), 20);
    raw.close().await.unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
async fn original_document_recovery_does_not_require_current_stock_counters() {
    let f = Fixture::new();
    let db = f.open().await;
    let stock = open(&db).await;
    let prepared = prepare(&db, &stock, 20).await;
    let issued = issue(&db, &prepared.record, 21).await;
    db.close().await.unwrap();
    let mut raw = f.raw().await;
    sqlx::query("UPDATE hub_stock_positions SET packs=19")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_stock_state SET event_count=0")
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let db = f.open().await;
    assert_eq!(
        db.hub_dispatch_operation(owner(), issued.operation.clone())
            .await
            .unwrap(),
        Some(issued)
    );
    assert_eq!(
        db.hub_dispatch_record(owner(), prepared.record.id)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_version_limit_preserves_original_issues_and_allows_unused_closure() {
    let f = Fixture::new();
    let db = f.open().await;
    let stock = open(&db).await;
    let prepared = prepare(&db, &stock, 20).await;
    let first = issue(&db, &prepared.record, 21).await;
    let mut current = first.clone();
    for version in 2..=32 {
        let input = command(
            &db,
            DispatchAction::Replace {
                preparation: current.record.id,
                expected_revision: current.record.revision,
                recipient: if version % 2 == 0 { id(9) } else { id(8) },
                lines: selection(&db, stock.original.position.group).await,
            },
        )
        .await;
        current = db
            .apply_hub_dispatch(owner(), operation(30 + version), input, now())
            .await
            .unwrap();
    }
    assert_eq!(current.record.versions.len(), 32);
    let blocked = command(
        &db,
        DispatchAction::Replace {
            preparation: current.record.id,
            expected_revision: current.record.revision,
            recipient: id(8),
            lines: selection(&db, stock.original.position.group).await,
        },
    )
    .await;
    assert_eq!(
        db.apply_hub_dispatch(owner(), operation(90), blocked, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubCapacity
    );
    let unused = command(
        &db,
        DispatchAction::MarkUnused {
            preparation: current.record.id,
            expected_revision: current.record.revision,
        },
    )
    .await;
    let closed = db
        .apply_hub_dispatch(owner(), operation(91), unused, now())
        .await
        .unwrap();
    assert_eq!(
        closed.record.versions.last().unwrap().status,
        IdentificationStatus::Unused
    );
    assert_eq!(
        closed.record.versions[0].material,
        first.record.versions[0].material
    );
    assert_eq!(
        db.hub_dispatch_operation(owner(), first.operation.clone())
            .await
            .unwrap(),
        Some(first)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_partial_selection_requires_separation_and_issue_never_releases_a_hold() {
    use harvestcircle_domain::hub::portions::SplitCommand;
    use harvestcircle_domain::hub::treatment::{
        TreatmentAction, TreatmentCommand, TreatmentSelection,
    };
    let f = Fixture::new();
    let db = f.open().await;
    let stock = open(&db).await;
    let root = stock.original.position.group;
    let group = db.hub_group(owner(), root).await.unwrap();
    let partial = DispatchSelection::new(vec![DispatchLineInput {
        group: root,
        expected_group_revision: group.group.revision,
        packs: WholePacks::from_value(5).unwrap(),
    }])
    .unwrap();
    let input = command(
        &db,
        DispatchAction::Prepare {
            recipient: id(8),
            lines: partial,
        },
    )
    .await;
    assert_eq!(
        db.apply_hub_dispatch(owner(), operation(20), input, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(
        db.hub_dispatch_page(owner(), query()).await.unwrap().total,
        0
    );
    let separated = db
        .apply_hub_split(
            owner(),
            operation(10),
            SplitCommand {
                expected_stock_revision: group.revision,
                expected_catalog_revision: group.catalog_revision,
                group: root,
                expected_group_revision: group.group.revision,
                packs: WholePacks::from_value(5).unwrap(),
                identification: PhysicalIdentification {
                    label: HubLabel::new("Green five").unwrap(),
                    observation: SourceProvenance::new(
                        "Five original crates in a separately tagged container",
                    )
                    .unwrap(),
                },
                occurred: Occurrence::Unknown,
                observation: SourceProvenance::new(
                    "Observed separately identified five at Receiving",
                )
                .unwrap(),
            },
            now(),
        )
        .await
        .unwrap();
    let HubHandlingResult::Separated(separated) = separated.result else {
        panic!("separation")
    };
    let five = separated.portion_group;
    let fresh = db.hub_group(owner(), five).await.unwrap();
    db.apply_hub_treatment(
        owner(),
        operation(11),
        TreatmentCommand {
            expected_stock_revision: fresh.revision,
            expected_catalog_revision: fresh.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![five]).unwrap(),
            },
            occurred: Occurrence::Unknown,
            observation: SourceProvenance::new("Independent inspection hold on these five")
                .unwrap(),
        },
        now(),
    )
    .await
    .unwrap();
    let input = command(
        &db,
        DispatchAction::Prepare {
            recipient: id(8),
            lines: selection(&db, five).await,
        },
    )
    .await;
    let prepared = db
        .apply_hub_dispatch(owner(), operation(21), input, now())
        .await
        .unwrap();
    let issued = issue(&db, &prepared.record, 22).await;
    assert_eq!(
        issued.record.versions[0].material.lines[0]
            .identification
            .as_ref()
            .unwrap()
            .label
            .as_str(),
        "Green five"
    );
    assert_eq!(issued.record.versions[0].material.total_packs().unwrap(), 5);
    let unused = command(
        &db,
        DispatchAction::MarkUnused {
            preparation: issued.record.id,
            expected_revision: issued.record.revision,
        },
    )
    .await;
    db.apply_hub_dispatch(owner(), operation(23), unused, now())
        .await
        .unwrap();
    let treatment = db
        .hub_treatment(
            owner(),
            crate::HubTreatmentScope {
                trace: stock.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(treatment.projection.summary.restricted.value(), 5);
    assert_eq!(treatment.projection.summary.on_hand.value(), 20);
    assert_eq!(treatment.holds.len(), 1);
    assert!(treatment.holds[0].hold.release.is_none());
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_original_digest_detects_valid_shaped_historical_recipient_corruption() {
    let f = Fixture::new();
    let db = f.open().await;
    let stock = open(&db).await;
    let prepared = prepare(&db, &stock, 20).await;
    let issued = issue(&db, &prepared.record, 21).await;
    db.close().await.unwrap();
    let mut raw = f.raw().await;
    sqlx::query("DROP TRIGGER hub_reference_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_reference_versions SET label='Changed historical recipient' WHERE record_id=? AND record_revision=1").bind(id(8).as_bytes().as_slice()).execute(&mut raw).await.unwrap();
    let guard = crate::hub::reference::MIGRATION_SQL
        .split(";\n")
        .find(|sql| sql.starts_with("CREATE TRIGGER hub_reference_no_update "))
        .unwrap();
    sqlx::query(guard).execute(&mut raw).await.unwrap();
    raw.close().await.unwrap();
    let db = f.open().await;
    assert_eq!(
        db.hub_dispatch_operation(owner(), issued.operation)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    db.close().await.unwrap();
}
