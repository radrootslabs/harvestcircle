use super::*;
use sqlx::{AssertSqlSafe, Connection};

#[tokio::test]
async fn native_count_epoch_and_marker_faults_reject_exact_e_and_c_originals_without_writes() {
    for explanation_target in [true, false] {
        for fault in 0..4 {
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
            assert_eq!(
                db.hub_count_explanation_operation(owner(), operation(801))
                    .await
                    .unwrap()
                    .as_ref(),
                Some(explanation.as_ref())
            );
            assert_eq!(
                db.hub_count_classification_operation(owner(), operation(803))
                    .await
                    .unwrap()
                    .as_ref(),
                Some(classification.as_ref())
            );
            let target = if explanation_target {
                operation(801)
            } else {
                operation(803)
            };
            let (guard, update) = match fault {
                0 => (
                    "hub_count_interpretation_markers_no_delete",
                    "DELETE FROM hub_count_interpretation_markers WHERE operation_id=?",
                ),
                1 => (
                    "hub_count_interpretation_markers_no_update",
                    "UPDATE hub_count_interpretation_markers SET capture_hash=zeroblob(32) WHERE operation_id=?",
                ),
                2 => (
                    "hub_count_interpretation_markers_no_update",
                    "UPDATE hub_count_interpretation_markers SET stock_revision=stock_revision+1 WHERE operation_id=?",
                ),
                3 => (
                    "hub_count_interpretation_epochs_no_update",
                    "UPDATE hub_count_interpretation_epochs SET legacy_stock_revision=legacy_stock_revision+1 WHERE hub_id=(SELECT hub_id FROM hub_stock_operations WHERE operation_id=?)",
                ),
                _ => unreachable!(),
            };
            let mut raw = fixture.raw().await;
            assert!(
                sqlx::query(update)
                    .bind(target.as_str())
                    .execute(&mut raw)
                    .await
                    .is_err()
            );
            let ddl: String =
                sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name=? AND type='trigger'")
                    .bind(guard)
                    .fetch_one(&mut raw)
                    .await
                    .unwrap();
            let mut tx = raw.begin().await.unwrap();
            if fault == 3 {
                // Keep the epoch/marker FK tuples consistent while corrupting
                // the epoch's stock/catalog dependency pair. This coordinated
                // mutation does not claim an isolated Return-parent fault.
                sqlx::query("PRAGMA defer_foreign_keys=ON")
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
            sqlx::query(AssertSqlSafe(format!("DROP TRIGGER {guard}")))
                .execute(&mut *tx)
                .await
                .unwrap();
            assert_eq!(
                sqlx::query(update)
                    .bind(target.as_str())
                    .execute(&mut *tx)
                    .await
                    .unwrap()
                    .rows_affected(),
                1
            );
            if fault == 3 {
                let marker_ddl: String = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name='hub_count_interpretation_markers_no_update'").fetch_one(&mut *tx).await.unwrap();
                sqlx::query("DROP TRIGGER hub_count_interpretation_markers_no_update")
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                assert_eq!(sqlx::query("UPDATE hub_count_interpretation_markers SET legacy_stock_revision=legacy_stock_revision+1").execute(&mut *tx).await.unwrap().rows_affected(), 2);
                sqlx::query(AssertSqlSafe(marker_ddl))
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
            sqlx::query(AssertSqlSafe(ddl))
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            if fault == 3 {
                let (catalog, dependency): (i64, i64) = sqlx::query_as("SELECT e.legacy_catalog_revision,COALESCE((SELECT max(o.catalog_revision) FROM hub_stock_operations o WHERE o.hub_id=e.hub_id AND o.stock_revision<=e.legacy_stock_revision),1) FROM hub_count_interpretation_epochs e")
                    .fetch_one(&mut raw).await.unwrap();
                assert!(
                    dependency > catalog,
                    "the corrupted cut has a newer catalog dependency"
                );
            }
            assert!(
                sqlx::query("PRAGMA foreign_key_check")
                    .fetch_all(&mut raw)
                    .await
                    .unwrap()
                    .is_empty()
            );
            raw.close().await.unwrap();
            let before = crate::hub::custody::tests::material(&fixture).await;
            assert_corrupt_original(&db, explanation_target).await;
            assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
            db.close().await.unwrap();
            let db = fixture.open().await;
            assert_corrupt_original(&db, explanation_target).await;
            assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
            db.close().await.unwrap();
        }
    }
}

async fn assert_corrupt_original(db: &Database, explanation: bool) {
    let error = if explanation {
        db.hub_count_explanation_operation(owner(), operation(801))
            .await
            .unwrap_err()
    } else {
        db.hub_count_classification_operation(owner(), operation(803))
            .await
            .unwrap_err()
    };
    assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
}
