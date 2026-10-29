use super::*;
use crate::hub::stock::tests::{
    Fixture, command as receipt_command, now, operation, other, owner, query, setup,
};
use harvestcircle_domain::SafeErrorCode;
use harvestcircle_domain::hub::reference::WholePacks;
use harvestcircle_domain::hub::stock::Occurrence;

fn command(
    catalog: &harvestcircle_domain::hub::reference::ReferenceCatalog,
    group: HubRecordId,
) -> CountObserveCommand {
    CountObserveCommand {
        expected_stock_revision: HubRevision::from_value(2).unwrap(),
        expected_catalog_revision: catalog.revision(),
        group,
        expected_group_revision: HubRevision::INITIAL,
        counted_packs: WholePacks::from_value(18).unwrap(),
        cutoff: Occurrence::Known(now()),
        observation: SourceProvenance::new(
            "Observed eighteen fictional crates; recorded balance is twenty",
        )
        .unwrap(),
    }
}

#[tokio::test]
async fn saving_eighteen_preserves_twenty_and_original_work_across_replay_and_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let group = before.rows[0].position.group;
    let input = command(&catalog, group);
    let saved = db
        .observe_hub_count(owner(), operation(2), input.clone(), now())
        .await
        .unwrap();
    assert_eq!(saved.original.scope.recorded_packs.value(), 20);
    assert_eq!(saved.original.counted_packs.value(), 18);
    assert_eq!(saved.original.scope.latest_physical_recorded_at, now());
    let after = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(after.revision.value(), 3);
    assert_eq!(after.rows, before.rows);
    assert_eq!(
        after.rows[0]
            .put_away
            .as_ref()
            .unwrap()
            .quantity
            .remaining()
            .value(),
        20
    );
    assert_eq!(
        db.observe_hub_count(owner(), operation(2), input, now())
            .await
            .unwrap(),
        saved
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(receipt)
    );
    let history = db
        .hub_trace_history(
            owner(),
            crate::HubTraceHistoryQuery {
                trace: saved.original.scope.trace,
                expected_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    assert_eq!(history.rows.len(), 2);
    assert_eq!(
        history.rows[1].kind,
        crate::HubTraceHistoryKind::CountObserved
    );
    assert_eq!(history.rows[1].occurred, Some(saved.original.cutoff));
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(saved)
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), after);
    db.close().await.unwrap();
}

#[tokio::test]
async fn stale_foreign_and_changed_intents_cannot_record_or_replace_a_count() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let input = command(&catalog, before.rows[0].position.group);
    assert_eq!(
        db.observe_hub_count(other(), operation(2), input.clone(), now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut stale = input.clone();
    stale.expected_group_revision = HubRevision::from_value(2).unwrap();
    assert_eq!(
        db.observe_hub_count(owner(), operation(2), stale, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    let saved = db
        .observe_hub_count(owner(), operation(2), input.clone(), now())
        .await
        .unwrap();
    let mut changed = input.clone();
    changed.counted_packs = WholePacks::from_value(19).unwrap();
    assert_eq!(
        db.observe_hub_count(owner(), operation(2), changed, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.observe_hub_count(owner(), operation(3), input, now())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(saved)
    );
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(4))
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(1))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn zero_unknown_and_interval_counts_are_observations_without_inventory_effects() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    for (n, cutoff) in [
        Occurrence::Unknown,
        Occurrence::Interval {
            earliest: now(),
            latest: now(),
        },
        Occurrence::Known(now()),
    ]
    .into_iter()
    .enumerate()
    {
        let mut input = command(&catalog, before.rows[0].position.group);
        input.expected_stock_revision = HubRevision::from_value(2 + n as u64).unwrap();
        input.counted_packs = WholePacks::from_value(0).unwrap();
        input.cutoff = cutoff;
        let saved = db
            .observe_hub_count(owner(), operation(2 + n as u32), input, now())
            .await
            .unwrap();
        assert_eq!(saved.original.cutoff, cutoff);
        assert_eq!(saved.original.counted_packs.value(), 0);
        assert_eq!(
            db.hub_stock_page(owner(), query()).await.unwrap().rows,
            before.rows
        );
    }
    db.close().await.unwrap();
}

#[tokio::test]
async fn concurrent_observations_have_one_revision_winner_and_exact_original_replay() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let input = command(&catalog, page.rows[0].position.group);
    let (a, b) = tokio::join!(
        db.observe_hub_count(owner(), operation(2), input.clone(), now()),
        db.observe_hub_count(owner(), operation(3), input.clone(), now()),
    );
    let (saved, error) = match (a, b) {
        (Ok(saved), Err(error)) | (Err(error), Ok(saved)) => (saved, error),
        results => panic!("exactly one revision winner: {results:?}"),
    };
    assert_eq!(error.code(), SafeErrorCode::HubConflict);
    let (a, b) = tokio::join!(
        db.observe_hub_count(owner(), saved.operation.clone(), input.clone(), now()),
        db.observe_hub_count(owner(), saved.operation.clone(), input, now()),
    );
    assert_eq!(a.unwrap(), saved);
    assert_eq!(b.unwrap(), saved);
    let after = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(after.revision.value(), 3);
    assert_eq!(after.rows, page.rows);
    db.close().await.unwrap();
}

#[tokio::test]
async fn original_count_retains_captured_references_after_rename_and_physical_move() {
    use harvestcircle_domain::hub::reference::{
        HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue,
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let group = page.rows[0].position.group;
    let input = command(&catalog, group);
    let saved = db
        .observe_hub_count(owner(), operation(2), input.clone(), now())
        .await
        .unwrap();
    db.apply_hub_reference(
        owner(),
        operation(3),
        ReferenceCommand {
            expected_catalog_revision: catalog.revision(),
            mutation: ReferenceMutation::Revise {
                id: saved.original.scope.product.id,
                expected_record_revision: saved.original.scope.product.revision,
                value: ReferenceValue::Product {
                    label: HubLabel::new("New current product name").unwrap(),
                },
            },
        },
        now(),
    )
    .await
    .unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    db.apply_hub_movement(
        owner(),
        operation(4),
        harvestcircle_domain::hub::movement::MovementCommand {
            expected_stock_revision: saved.original.recorded_revision,
            expected_catalog_revision: catalog.revision(),
            group,
            expected_group_revision: HubRevision::INITIAL,
            packs: WholePacks::from_value(20).unwrap(),
            to: crate::hub::stock::tests::id(7),
            separated_portion: None,
            occurred: Occurrence::Known(now()),
            observation: SourceProvenance::new("Moved after the retained count observation")
                .unwrap(),
        },
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.observe_hub_count(owner(), operation(2), input, now())
            .await
            .unwrap(),
        saved
    );
    assert_eq!(
        saved.original.scope.product.value.label().as_str(),
        "Fictional apples"
    );
    assert_eq!(
        db.hub_group(owner(), group).await.unwrap().group.location,
        crate::hub::stock::tests::id(7)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn failure_after_all_count_writes_rolls_back_history_and_leaves_stock_unchanged() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    db.apply_hub_receipt(owner(), operation(1), receipt_command(&catalog), now())
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let input = command(&catalog, before.rows[0].position.group);
    let operation = operation(2);
    let hub = db.load_hub().await.unwrap().unwrap();
    let hash = input.fingerprint(hub.id(), owner(), operation_id(&operation).unwrap());
    let failed: Result<HubCountObservationReceipt, _> = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                write::observe_on(tx, hub.id(), owner(), operation, input, hash, now()).await?;
                Err(storage_unavailable())
            })
        })
        .await;
    assert!(failed.is_err());
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    assert!(
        db.hub_count_observation_operation(owner(), crate::hub::stock::tests::operation(2))
            .await
            .unwrap()
            .is_none()
    );
    let mut raw = fixture.raw().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM hub_count_observations")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        0
    );
    sqlx::Connection::close(raw).await.unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
async fn actual_confirmed_rollback_preserves_rejection_and_restores_native_state() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let rejected = stock::conflict();
    let error = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                sqlx::query("UPDATE runtime_state SET session_generation=31 WHERE singleton=1")
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| crate::db::storage_unavailable())?;
                Err::<(), _>(rejected)
            })
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.kind(),
        radroots_service_sqlite::ServiceSqliteTransactionErrorKind::OperationRolledBack
    );
    assert_eq!(crate::db::map_transaction_error(error), rejected);
    let mut raw = fixture.raw().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT session_generation FROM runtime_state WHERE singleton=1"
        )
        .fetch_one(&mut raw)
        .await
        .unwrap(),
        0
    );
    sqlx::Connection::close(raw).await.unwrap();
    db.close().await.unwrap();
}
