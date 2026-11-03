use super::*;
use crate::hub::stock::tests::{
    Fixture, command as intake_command, id, now, operation, owner, query, setup,
};
use harvestcircle_domain::SafeErrorCode;
use harvestcircle_domain::hub::movement::{
    MovementCommand, NewWorkCommand, ReplanCommand, WorkProgress,
};
use harvestcircle_domain::hub::reference::{
    HubLabel, LocationPurpose, ReferenceCommand, ReferenceMutation, ReferenceValue,
    SourceProvenance,
};
use harvestcircle_domain::hub::stock::Occurrence;
use sqlx::Connection;
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}
fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}
async fn command(db: &Database, group: HubRecordId) -> DispositionCommand {
    let current = db.hub_group(owner(), group).await.unwrap();
    DispositionCommand {
        expected_stock_revision: current.revision,
        expected_catalog_revision: current.catalog_revision,
        group,
        expected_group_revision: current.group.revision,
        packs: current.group.packs,
        kind: DispositionKind::Destroyed,
        occurred: Occurrence::Unknown,
        observation: note("Actually observed complete selected stock removal"),
    }
}
async fn stock(db: &Database) -> crate::HubStockReceipt {
    let catalog = setup(db).await;
    db.apply_hub_reference(
        owner(),
        operation(600),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Create {
                id: id(10),
                value: ReferenceValue::Location {
                    label: HubLabel::new("B3").unwrap(),
                    purpose: LocationPurpose::Storage,
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    db.apply_hub_receipt(owner(), operation(1), intake_command(&catalog), now())
        .await
        .unwrap()
}
#[tokio::test]
async fn actual_complete_disposition_retires_pending_without_departure_or_count_loss_and_replay_cannot_repeat()
 {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = stock(&db).await;
    let input = command(&db, intake.original.position.group).await;
    let mut partial = input.clone();
    partial.packs = packs(2);
    assert_eq!(
        db.apply_hub_disposition(owner(), operation(2), partial, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().rows[0]
            .position
            .packs,
        packs(20)
    );
    let result = db
        .apply_hub_disposition(owner(), operation(3), input.clone(), now())
        .await
        .unwrap();
    assert_eq!(result.original.consumed.before.packs, packs(20));
    assert_eq!(result.original.work_changes.len(), 1);
    let work = &result.original.work_changes[0];
    assert_eq!(work.after.quantity.completed(), packs(0));
    assert_eq!(work.after.quantity.no_longer_required(), packs(20));
    assert_eq!(work.after.quantity.remaining(), packs(0));
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.apply_hub_disposition(owner(), operation(3), input.clone(), now())
            .await
            .unwrap(),
        result
    );
    assert_eq!(
        db.apply_hub_disposition(owner(), operation(4), input, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(intake)
    );
    let mut raw = fixture.raw().await;
    for sql in [
        "SELECT count(*) FROM hub_departures",
        "SELECT count(*) FROM hub_count_reconciliations",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(&mut raw)
                .await
                .unwrap(),
            0
        );
    }
    raw.close().await.unwrap();
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_disposition_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(result)
    );
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn disposition_after_new_work_preserves_completed_history_and_accepts_distinct_group_position_revisions()
 {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = stock(&db).await;
    let group = intake.original.position.group;
    let before = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(2),
        MovementCommand {
            expected_stock_revision: before.revision,
            expected_catalog_revision: before.catalog_revision,
            group,
            expected_group_revision: before.group.revision,
            to: id(7),
            packs: packs(20),
            separated_portion: None,
            occurred: Occurrence::Unknown,
            observation: note("Actually put all twenty crates at A3"),
        },
        now(),
    )
    .await
    .unwrap();
    let current = db.hub_group(owner(), group).await.unwrap();
    db.apply_hub_new_work(
        owner(),
        operation(3),
        NewWorkCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            groups: vec![group],
            target: id(10),
            observation: note("New handling has not happened"),
        },
        now(),
    )
    .await
    .unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    let location = catalog.get(id(7)).unwrap();
    db.apply_hub_reference(
        owner(),
        operation(601),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: id(7),
                expected_record_revision: location.revision,
                value: ReferenceValue::Location {
                    label: HubLabel::new("Renamed A3 current label").unwrap(),
                    purpose: LocationPurpose::Storage,
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    let input = command(&db, group).await;
    let mut raw = fixture.raw().await;
    let position: i64 =
        sqlx::query_scalar("SELECT revision FROM hub_stock_positions WHERE group_id=?")
            .bind(group.as_bytes().as_slice())
            .fetch_one(&mut raw)
            .await
            .unwrap();
    raw.close().await.unwrap();
    assert!(input.expected_group_revision.value() > u64::try_from(position).unwrap());
    let result = db
        .apply_hub_disposition(owner(), operation(4), input, now())
        .await
        .unwrap();
    assert_eq!(
        result.original.location.value.label().as_str(),
        "Renamed A3 current label"
    );
    assert_eq!(
        result
            .original
            .consumed
            .before
            .work
            .iter()
            .filter(|a| a.progress == WorkProgress::Completed)
            .count(),
        1
    );
    assert_eq!(result.original.work_changes.len(), 1);
    assert_eq!(
        result.original.work_changes[0].after.quantity.completed(),
        packs(0)
    );
    assert_eq!(result.original.work_changes[0].retired, packs(20));
    let original_completed = result
        .original
        .consumed
        .before
        .work
        .iter()
        .find(|a| a.progress == WorkProgress::Completed)
        .unwrap();
    assert_eq!(
        original_completed.requirement.quantity.completed(),
        packs(20)
    );
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn disposition_after_replan_preserves_retired_original_and_retires_only_fresh_pending_work() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = stock(&db).await;
    let group = intake.original.position.group;
    let current = db.hub_group(owner(), group).await.unwrap();
    let work = &current.group.work[0].requirement;
    db.apply_hub_replan(
        owner(),
        operation(2),
        ReplanCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            requirement: work.id,
            expected_work_revision: work.revision,
            target: id(10),
            observation: note("Replan only; original pending work retired without completion"),
        },
        now(),
    )
    .await
    .unwrap();
    let input = command(&db, group).await;
    let mut raw = fixture.raw().await;
    let position: i64 =
        sqlx::query_scalar("SELECT revision FROM hub_stock_positions WHERE group_id=?")
            .bind(group.as_bytes().as_slice())
            .fetch_one(&mut raw)
            .await
            .unwrap();
    raw.close().await.unwrap();
    assert!(input.expected_group_revision.value() > u64::try_from(position).unwrap());
    let result = db
        .apply_hub_disposition(owner(), operation(3), input, now())
        .await
        .unwrap();
    assert_eq!(
        result
            .original
            .consumed
            .before
            .work
            .iter()
            .filter(|a| a.progress == WorkProgress::NoLongerRequired)
            .count(),
        1
    );
    assert_eq!(result.original.work_changes.len(), 1);
    assert_eq!(
        result.original.work_changes[0].before.quantity.remaining(),
        packs(20)
    );
    assert_eq!(
        result.original.work_changes[0]
            .after
            .quantity
            .no_longer_required(),
        packs(20)
    );
    assert_eq!(
        result.original.work_changes[0].after.quantity.completed(),
        packs(0)
    );
    assert!(
        db.hub_stock_page(owner(), query())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_trace_snapshot_and_history_keep_disposition_separate_from_departure_and_loss() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = stock(&db).await;
    let input = command(&db, intake.original.position.group).await;
    let result = db
        .apply_hub_disposition(owner(), operation(2), input, now())
        .await
        .unwrap();
    let snapshot = db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: intake.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let balance = snapshot.balance().unwrap();
    assert_eq!(balance.on_hand, packs(0));
    assert_eq!(balance.departed, packs(0));
    assert_eq!(balance.counted_loss, packs(0));
    assert_eq!(balance.disposed, packs(20));
    assert_eq!(snapshot.dispositions.len(), 1);
    assert_eq!(
        snapshot.dispositions[0].id,
        result.original.identities.disposition
    );
    let history = db
        .hub_trace_history(
            owner(),
            crate::HubTraceHistoryQuery {
                trace: intake.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    assert!(
        history
            .rows
            .iter()
            .any(|r| r.kind == crate::HubTraceHistoryKind::Disposition)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn disposition_late_failure_rolls_back_originals_positions_work_events_and_revisions() {
    use crate::hub::custody::tests::material;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = stock(&db).await;
    let input = command(&db, intake.original.position.group).await;
    let before = material(&fixture).await;
    let attempted = input.clone();
    assert!(
        db.host()
            .transaction(|tx| Box::pin(async move {
                // Inject failure only after the complete production writer and its readback.
                write::apply_on(tx, owner(), operation(2), attempted, now()).await?;
                Err::<(), SafeError>(storage_unavailable())
            }))
            .await
            .is_err()
    );
    assert_eq!(material(&fixture).await, before);
    assert!(
        db.hub_disposition_operation(owner(), operation(2))
            .await
            .unwrap()
            .is_none()
    );
    let result = db
        .apply_hub_disposition(owner(), operation(2), input.clone(), now())
        .await
        .unwrap();
    assert_eq!(
        db.apply_hub_disposition(owner(), operation(2), input, now())
            .await
            .unwrap(),
        result
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn disposition_corrupt_full_seal_or_scope_never_becomes_a_valid_original_or_trace() {
    use crate::hub::custody::tests::{corrupt, material};
    for (trigger, sql) in [
        (
            "hub_dispositions_no_update",
            "UPDATE hub_dispositions SET result_hash=zeroblob(32)",
        ),
        (
            "hub_disposition_scope_no_update",
            "UPDATE hub_disposition_work_scope SET before_remaining=before_remaining-1",
        ),
    ] {
        let fixture = Fixture::new();
        let db = fixture.open().await;
        let intake = stock(&db).await;
        let input = command(&db, intake.original.position.group).await;
        db.apply_hub_disposition(owner(), operation(2), input.clone(), now())
            .await
            .unwrap();
        corrupt(&fixture, trigger, sql).await;
        let before = material(&fixture).await;
        assert_eq!(
            db.hub_disposition_operation(owner(), operation(2))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            db.apply_hub_disposition(owner(), operation(2), input, now())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            db.hub_trace_snapshot(
                owner(),
                crate::HubTraceSnapshotQuery {
                    trace: intake.original.trace.id,
                    expected_revision: None,
                    expected_catalog_revision: None,
                }
            )
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn disposition_foreign_and_stale_requests_leave_every_native_row_unchanged() {
    use crate::hub::custody::tests::material;
    use crate::hub::stock::tests::other;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = stock(&db).await;
    let input = command(&db, intake.original.position.group).await;
    let before = material(&fixture).await;
    assert!(
        db.apply_hub_disposition(other(), operation(2), input.clone(), now())
            .await
            .is_err()
    );
    assert_eq!(material(&fixture).await, before);
    db.apply_hub_disposition(owner(), operation(2), input.clone(), now())
        .await
        .unwrap();
    let after = material(&fixture).await;
    assert_eq!(
        db.apply_hub_disposition(owner(), operation(3), input, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(material(&fixture).await, after);
    db.close().await.unwrap();
}
