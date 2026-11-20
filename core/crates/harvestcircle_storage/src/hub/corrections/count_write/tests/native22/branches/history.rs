use super::*;
use harvestcircle_domain::hub::counts::CountPostPlan;
use sqlx::{AssertSqlSafe, Connection};

pub(super) fn qualify_foreign_suffix<'a>(
    fixture: &'a Fixture,
    expected: Box<crate::HubCountExplanationReceipt>,
    count_b: Box<CountPostPlan>,
    old_cut: HubRevision,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        let mut raw = fixture.raw().await;
        let bad = operation(20013);
        let account_b = crate::hub::actuals::common::operation(count_b.operation).unwrap();
        let actual_account: String = sqlx::query_scalar(
            "SELECT account_operation_id FROM hub_corrections WHERE operation_id=?",
        )
        .bind(bad.as_str())
        .fetch_one(&mut raw)
        .await
        .unwrap();
        assert_eq!(actual_account, account_b.as_str());
        let revision: i64 = sqlx::query_scalar(
            "SELECT stock_revision FROM hub_stock_operations WHERE operation_id=?",
        )
        .bind(bad.as_str())
        .fetch_one(&mut raw)
        .await
        .unwrap();
        let bad_cut = HubRevision::from_value(u64::try_from(revision).unwrap()).unwrap();
        let warm_cut = HubRevision::from_value(bad_cut.value() - 1).unwrap();
        let accounts:Vec<String>=sqlx::query_scalar("SELECT c.account_operation_id FROM hub_corrections c JOIN hub_stock_operations o ON o.operation_id=c.operation_id WHERE c.family IN ('count_explain','count_classify') AND o.stock_revision>? AND o.stock_revision<=?")
            .bind(i64::try_from(old_cut.value()).unwrap()).bind(i64::try_from(warm_cut.value()).unwrap())
            .fetch_all(&mut raw).await.unwrap();
        assert!(
            !accounts.is_empty() && accounts.iter().all(|account| account == account_b.as_str())
        );
        let change = "UPDATE hub_corrections SET subject_id=? WHERE operation_id=?";
        assert!(
            sqlx::query(change)
                .bind([7_u8; 16].as_slice())
                .bind(bad.as_str())
                .execute(&mut raw)
                .await
                .is_err()
        );
        let ddl: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE name='hub_corrections_no_update'",
        )
        .fetch_one(&mut raw)
        .await
        .unwrap();
        sqlx::query("DROP TRIGGER hub_corrections_no_update")
            .execute(&mut raw)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query(change)
                .bind([7_u8; 16].as_slice())
                .bind(bad.as_str())
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
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut raw)
                .await
                .unwrap()
                .is_empty()
        );
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(fixture).await;
        let db = fixture.open().await;
        let hub = db.load_hub().await.unwrap().unwrap().id();
        let count_a = Box::new(expected.original.count.clone());
        db.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let trace_a = count_a.before.trace;
                    let mut cold = OriginalReader::new(tx);
                    cold.test_count_row_limit(0);
                    let old_rows = cold.correction_rows_on(hub, trace_a, old_cut).await?;
                    let warm_rows = cold.correction_rows_on(hub, trace_a, warm_cut).await?;
                    drop(cold);
                    for (slots, rows) in [(0, 0), (1, 4096), (4096, 2), (4096, 4096)] {
                        let mut reader = OriginalReader::new(tx);
                        reader.test_history_limits(slots, rows);
                        reader.test_count_row_limit(slots);
                        assert_eq!(
                            reader.correction_rows_on(hub, trace_a, old_cut).await?,
                            old_rows
                        );
                        assert_eq!(
                            reader.correction_rows_on(hub, trace_a, warm_cut).await?,
                            warm_rows
                        );
                        let first = reader.count_history_on(hub, &count_a, old_cut).await?;
                        assert_eq!((first.explanations.len(), first.amendments.len()), (1, 1));
                        let only_b = reader.count_history_on(hub, &count_a, warm_cut).await?;
                        assert_eq!(only_b, first);
                        if slots > 0 && rows >= 2 {
                            assert_eq!(reader.test_history_stats().2, Some(warm_cut));
                        }
                        let other = reader.count_history_on(hub, &count_b, warm_cut).await?;
                        assert_eq!((other.explanations.len(), other.amendments.len()), (1, 0));
                        assert_ne!(other.original.trace, first.original.trace);
                        assert_eq!(
                            reader.count_history_on(hub, &count_a, warm_cut).await?,
                            first
                        );
                        let checkpoint = reader.test_history_stats();
                        let row_checkpoint = reader.test_count_row_points();
                        for _ in 0..2 {
                            assert_eq!(
                                reader
                                    .correction_rows_on(hub, trace_a, bad_cut)
                                    .await
                                    .unwrap_err()
                                    .code(),
                                SafeErrorCode::StorageCorrupt
                            );
                            assert_eq!(reader.test_count_row_points(), row_checkpoint);
                            assert_eq!(
                                reader
                                    .count_history_on(hub, &count_a, bad_cut)
                                    .await
                                    .unwrap_err()
                                    .code(),
                                SafeErrorCode::StorageCorrupt
                            );
                            assert_eq!(reader.test_history_stats(), checkpoint);
                            assert_eq!(
                                reader.count_history_on(hub, &count_a, old_cut).await?,
                                first
                            );
                            assert_eq!(
                                reader.count_history_on(hub, &count_a, warm_cut).await?,
                                first
                            );
                        }
                        assert_eq!(
                            reader.correction_rows_on(hub, trace_a, old_cut).await?,
                            old_rows
                        );
                        assert_eq!(
                            reader.correction_rows_on(hub, trace_a, warm_cut).await?,
                            warm_rows
                        );
                    }
                    Ok::<_, SafeError>(())
                })
            })
            .await
            .unwrap();
        assert_eq!(
            db.hub_count_explanation_operation(owner(), operation(801))
                .await
                .unwrap()
                .as_ref(),
            Some(expected.as_ref())
        );
        assert_eq!(crate::hub::custody::tests::material(fixture).await, before);
        db.close().await.unwrap();
    })
}
