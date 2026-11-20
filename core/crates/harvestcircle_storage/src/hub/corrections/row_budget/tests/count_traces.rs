use super::*;
use sqlx::AssertSqlSafe;

#[tokio::test]
async fn scan_local_count_trace_reuse_checks_every_canonical_header_field() {
    let (fixture, db, _, receipt) =
        super::super::super::count_review::tests::explained_world().await;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let account = crate::hub::actuals::common::operation(receipt.original.count.operation).unwrap();
    let trace = receipt.original.count.before.trace;
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut memo = CountTraces::new();
                for _ in 0..2 {
                    assert_eq!(count_trace_on(tx, hub, &account, &mut memo).await?, trace);
                }
                assert_eq!(memo.len(), 1);
                let saved = memo.get(account.as_str()).cloned().unwrap();
                for field in 0..6 {
                    let mut forged = saved.clone();
                    match field {
                        0 => forged.0.actor[0] ^= 1,
                        1 => forged.0.intent[0] ^= 1,
                        2 => forged.0.stock += 1,
                        3 => forged.0.catalog += 1,
                        4 => forged.0.result[0] ^= 1,
                        _ => forged.0.recorded += 1,
                    }
                    memo.insert(account.as_str().to_owned(), forged);
                    assert_eq!(
                        count_trace_on(tx, hub, &account, &mut memo)
                            .await
                            .unwrap_err()
                            .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    memo.insert(account.as_str().to_owned(), saved.clone());
                    assert_eq!(count_trace_on(tx, hub, &account, &mut memo).await?, trace);
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn failed_count_post_original_never_enters_the_scan_local_trace_memo() {
    let (fixture, db, _, receipt) =
        super::super::super::count_review::tests::explained_world().await;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let account = crate::hub::actuals::common::operation(receipt.original.count.operation).unwrap();
    db.close().await.unwrap();
    let mut raw = fixture.raw().await;
    let change =
        "UPDATE hub_count_reconciliations SET result_hash=zeroblob(32) WHERE operation_id=?";
    assert!(
        sqlx::query(change)
            .bind(account.as_str())
            .execute(&mut raw)
            .await
            .is_err()
    );
    let ddl: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE name='hub_count_reconciliation_no_update'",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    sqlx::query("DROP TRIGGER hub_count_reconciliation_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query(change)
            .bind(account.as_str())
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
    let db = fixture.open().await;
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut memo = CountTraces::new();
                for _ in 0..2 {
                    assert_eq!(
                        count_trace_on(tx, hub, &account, &mut memo)
                            .await
                            .unwrap_err()
                            .code(),
                        SafeErrorCode::StorageCorrupt
                    );
                    assert!(memo.is_empty());
                }
                Ok::<_, SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
