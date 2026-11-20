use super::*;
use sqlx::{AssertSqlSafe, Connection};

#[tokio::test]
async fn completed_and_evicted_count_known_match_without_physical_changes() {
    let (fixture, db, _, expected) =
        super::super::super::super::count_review::tests::explained_world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                for limit in [4096, 0] {
                    let mut reader = OriginalReader::for_test_budget(tx, limit, 64 * 1024 * 1024);
                    for _ in 0..2 {
                        let op = stock::read::operation_on(&mut reader, hub, &operation(801))
                            .await?
                            .unwrap();
                        let known = super::super::super::super::count_known::original_on(
                            &mut reader,
                            hub,
                            op,
                        )
                        .await?;
                        assert_eq!(known, expected.original.known_event);
                    }
                    assert_eq!(reader.proof_counts().0, 1);
                    assert_eq!(reader.proof_counts().2 > 0, limit > 0);
                    let mut forged = stock::read::operation_on(&mut reader, hub, &operation(801))
                        .await?
                        .unwrap();
                    forged.hash[0] ^= 1;
                    assert_eq!(
                        super::super::super::super::count_known::original_on(
                            &mut reader,
                            hub,
                            forged
                        )
                        .await
                        .unwrap_err()
                        .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                }
                let mut reader = OriginalReader::new(tx);
                let op = stock::read::operation_on(&mut reader, hub, &operation(801))
                    .await?
                    .unwrap();
                super::super::super::super::count_known::original_on(&mut reader, hub, op).await?;
                sqlx::query("UPDATE hub_stock_state SET event_count=event_count WHERE singleton=1")
                    .execute(&mut *reader)
                    .await
                    .unwrap();
                let op = stock::read::operation_on(&mut reader, hub, &operation(801))
                    .await?
                    .unwrap();
                assert_eq!(
                    super::super::super::super::count_known::original_on(&mut reader, hub, op)
                        .await
                        .unwrap_err()
                        .code(),
                    SafeErrorCode::StorageCorrupt
                );
                drop(reader);
                let mut fresh = OriginalReader::new(tx);
                let op = stock::read::operation_on(&mut fresh, hub, &operation(801))
                    .await?
                    .unwrap();
                assert_eq!(
                    super::super::super::super::count_known::original_on(&mut fresh, hub, op)
                        .await?,
                    expected.original.known_event
                );
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn chronological_prefix_excludes_future_corruption_and_rejects_it_when_included() {
    let (fixture, db, scope, explanation) =
        super::super::super::super::count_review::tests::explained_world().await;
    let classification = classify(
        &db,
        scope,
        explanation.original.known_event.trace,
        id(801),
        id(801),
        0,
        (802, 210),
    )
    .await;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let mut raw = fixture.raw().await;
    let update = "UPDATE hub_corrections SET result_hash=zeroblob(32) WHERE operation_id=?";
    assert!(
        sqlx::query(update)
            .bind(operation(803).as_str())
            .execute(&mut raw)
            .await
            .is_err()
    );
    let ddl: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name='hub_corrections_no_update'")
            .fetch_one(&mut raw)
            .await
            .unwrap();
    sqlx::query(AssertSqlSafe(
        "DROP TRIGGER hub_corrections_no_update".to_owned(),
    ))
    .execute(&mut raw)
    .await
    .unwrap();
    assert_eq!(
        sqlx::query(update)
            .bind(operation(803).as_str())
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    sqlx::query(AssertSqlSafe(ddl))
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut reader = OriginalReader::new(tx);
                super::super::super::super::count_headers::through_on(
                    &mut reader,
                    hub,
                    explanation.original.explanation.stock_revision,
                )
                .await?;
                let checkpoint = reader.count_prefix_checkpoint();
                for _ in 0..2 {
                    assert_eq!(
                        super::super::super::super::count_headers::on(&mut reader, hub)
                            .await
                            .unwrap_err()
                            .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    assert_eq!(reader.count_prefix_checkpoint(), checkpoint);
                    assert_eq!(
                        super::super::super::super::count_headers::through_on(
                            &mut reader,
                            hub,
                            classification.original.amendment.stock_revision
                        )
                        .await
                        .unwrap_err()
                        .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    assert_eq!(reader.count_prefix_checkpoint(), checkpoint);
                    super::super::super::super::count_headers::through_on(
                        &mut reader,
                        hub,
                        explanation.original.explanation.stock_revision,
                    )
                    .await?;
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_count_classification_operation(owner(), operation(803))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
