use super::*;

#[tokio::test]
async fn restored_classification_and_final_state_share_only_the_fresh_postwrite_phase() {
    let (fixture, db, scope, _) = corrections::count_review::tests::explained_world().await;
    let own = |n| crate::hub::actuals::common::operation_id(&operation(n)).unwrap();
    let expected = corrections::count_return_write::tests::native22::classification(
        &db,
        scope,
        own(801),
        (own(801), None),
        0,
        802,
        210,
    )
    .await
    .unwrap();
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let before = crate::hub::custody::tests::material_digest(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let original = stock::read::state_on(tx, hub).await?;
                for entries in [4096, 0] {
                    let mut reader = OriginalReader::for_test_budget(tx, entries, 64 * 1024 * 1024);
                    let op = stock::read::operation_on(&mut reader, hub, &operation(803))
                        .await?
                        .unwrap();
                    let restored = reader.count_classification(hub, op).await?;
                    assert_eq!(*restored, *expected);
                    let rebuilt = reader.proof_counts().0;
                    assert!(rebuilt > 0);
                    let state = stock::read::state_with_proofs_on(&mut reader, hub).await?;
                    assert_eq!(
                        (state.revision, state.event_count),
                        (original.revision, original.event_count)
                    );
                    assert_eq!(reader.proof_counts().0, rebuilt);
                    sqlx::query(
                        "UPDATE hub_stock_state SET event_count=event_count WHERE singleton=1",
                    )
                    .execute(&mut *reader)
                    .await
                    .unwrap();
                    assert_eq!(
                        stock::read::state_with_proofs_on(&mut reader, hub)
                            .await
                            .err()
                            .unwrap()
                            .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    drop(reader);
                    let mut fresh = OriginalReader::for_test_budget(tx, entries, 64 * 1024 * 1024);
                    let state = stock::read::state_with_proofs_on(&mut fresh, hub).await?;
                    assert_eq!(
                        (state.revision, state.event_count),
                        (original.revision, original.event_count)
                    );
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(
        crate::hub::custody::tests::material_digest(&fixture).await,
        before
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn stock_state_and_count_prefix_share_one_phase_without_duplicate_full_originals() {
    let (fixture, db, _, _) = corrections::count_review::tests::explained_world().await;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let original = stock::read::state_on(tx, hub).await?;
                for entries in [4096, 0] {
                    let mut reader = OriginalReader::for_test_budget(tx, entries, 64 * 1024 * 1024);
                    let shared = stock::read::state_with_proofs_on(&mut reader, hub).await?;
                    assert_eq!(
                        (shared.revision, shared.event_count),
                        (original.revision, original.event_count)
                    );
                    let rebuilt = reader.proof_counts().0;
                    assert!(rebuilt > 0);
                    corrections::test_count_prefix_on(&mut reader, hub, shared.revision).await?;
                    assert_eq!(reader.proof_counts().0, rebuilt);
                    crate::hub::handling::treatment::capacity::ensure_with_proofs_on(
                        &mut reader,
                        owner(),
                        &operation(802),
                        crate::hub::handling::treatment::capacity::Reservation::OrdinaryMetadata,
                    )
                    .await?;
                    assert_eq!(reader.proof_counts().0, rebuilt);
                    assert_eq!(crate::hub::handling::treatment::capacity::ensure_with_proofs_on(
                        &mut reader, stock::tests::other(), &operation(802),
                        crate::hub::handling::treatment::capacity::Reservation::OrdinaryMetadata,
                    ).await.err().unwrap().code(), SafeErrorCode::HubPermissionDenied);
                    sqlx::query(
                        "UPDATE hub_stock_state SET event_count=event_count WHERE singleton=1",
                    )
                    .execute(&mut *reader)
                    .await
                    .unwrap();
                    assert_eq!(
                        stock::read::state_with_proofs_on(&mut reader, hub)
                            .await
                            .err()
                            .unwrap()
                            .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    drop(reader);
                    let mut fresh = OriginalReader::for_test_budget(tx, entries, 64 * 1024 * 1024);
                    let state = stock::read::state_with_proofs_on(&mut fresh, hub).await?;
                    assert_eq!(
                        (state.revision, state.event_count),
                        (original.revision, original.event_count)
                    );
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
