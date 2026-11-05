//! Fresh disposable corruption worlds; no historical witness is opened.
use crate::hub::{
    corrections::projection_native_tests,
    stock::tests::{Fixture, owner},
};
use harvestcircle_domain::SafeErrorCode;
use sqlx::{AssertSqlSafe, Connection, SqliteConnection};

async fn drop_guard(raw: &mut SqliteConnection, name: &str) -> String {
    let sql: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?")
            .bind(name)
            .fetch_one(&mut *raw)
            .await
            .unwrap();
    sqlx::query(AssertSqlSafe(format!("DROP TRIGGER {name}")))
        .execute(raw)
        .await
        .unwrap();
    sql
}

#[tokio::test]
async fn native20_missing_resealed_marker_epoch_and_capture_cannot_downgrade_a_qualified_original()
{
    for linked in [false, true] {
        for fault in 0..5 {
            let (fixture, db, operation) = if linked {
                let (fixture, db, scope, command) = super::unlinked_world().await;
                let operation = crate::hub::stock::tests::operation(18001);
                let now = harvestcircle_domain::UnixTimestamp::from_seconds(190).unwrap();
                db.submit_hub_mutation(
                    scope,
                    operation.clone(),
                    harvestcircle_application::hub_submission::HubMutation::ActualLink(Box::new(
                        command,
                    )),
                    now,
                )
                .await
                .unwrap();
                (fixture, db, operation)
            } else {
                let fixture = Fixture::new();
                let db = fixture.open().await;
                let (_, operation, _) = projection_native_tests::actual_debit(&db).await;
                (fixture, db, operation)
            };
            let original = db
                .hub_actual_review_operation(owner(), operation.clone())
                .await
                .unwrap()
                .unwrap();
            let mut raw = fixture.raw().await;
            let mut guards = Vec::new();
            match fault {
                0 | 2 | 4 => {
                    // Ordinary SQL cannot remove a required marker.
                    assert!(
                        sqlx::query(
                            "DELETE FROM hub_actual_interpretation_markers WHERE operation_id=?"
                        )
                        .bind(operation.as_str())
                        .execute(&mut raw)
                        .await
                        .is_err()
                    );
                    guards.push(
                        drop_guard(&mut raw, "hub_actual_interpretation_markers_no_delete").await,
                    );
                    sqlx::query(
                        "DELETE FROM hub_actual_interpretation_markers WHERE operation_id=?",
                    )
                    .bind(operation.as_str())
                    .execute(&mut raw)
                    .await
                    .unwrap();
                    if fault == 2 || fault == 4 {
                        sqlx::query("DELETE FROM hub_actual_interpretation_markers")
                            .execute(&mut raw)
                            .await
                            .unwrap();
                    }
                    if fault == 2 {
                        guards.push(
                            drop_guard(&mut raw, "hub_actual_interpretation_epochs_no_delete")
                                .await,
                        );
                        sqlx::query("DELETE FROM hub_actual_interpretation_epochs")
                            .execute(&mut raw)
                            .await
                            .unwrap();
                    } else if fault == 4 {
                        // Relabeling a new qualified seal as old mode still fails.
                        guards.push(
                            drop_guard(&mut raw, "hub_actual_interpretation_epochs_no_update")
                                .await,
                        );
                        sqlx::query(
                            "UPDATE hub_actual_interpretation_epochs SET legacy_stock_revision=?",
                        )
                        .bind(i64::try_from(original.revision.value()).unwrap())
                        .execute(&mut raw)
                        .await
                        .unwrap();
                    }
                }
                1 => {
                    guards.push(
                        drop_guard(&mut raw, "hub_actual_interpretation_markers_no_update").await,
                    );
                    sqlx::query("UPDATE hub_actual_interpretation_markers SET capture_hash=zeroblob(32) WHERE operation_id=?")
                    .bind(operation.as_str()).execute(&mut raw).await.unwrap();
                }
                3 => {
                    let bytes: Vec<u8> = sqlx::query_scalar(
                        "SELECT capture_body FROM hub_submissions WHERE operation_id=?",
                    )
                    .bind(operation.as_str())
                    .fetch_one(&mut raw)
                    .await
                    .unwrap();
                    let text = String::from_utf8(bytes).unwrap();
                    let prefix = if linked {
                        ",\"interpretation\":"
                    } else {
                        ",\"interpretations\":"
                    };
                    let start = text.find(prefix).unwrap();
                    // Inspect data only, keeping exact original request bytes/order.
                    // No archived input is decoded into an executable command.
                    let mut decoder =
                        serde_json::Deserializer::from_str(&text[start + prefix.len()..]);
                    let value: Box<serde_json::value::RawValue> =
                        serde::Deserialize::deserialize(&mut decoder).unwrap();
                    let mut forged = text.clone();
                    forged.replace_range(start..start + prefix.len() + value.get().len(), "");
                    let metadata =
                        harvestcircle_application::hub_submission::inspect_hub_submission(
                            forged.as_bytes(),
                        )
                        .unwrap();
                    guards.push(drop_guard(&mut raw, "hub_submissions_no_update").await);
                    guards.push(
                        drop_guard(&mut raw, "hub_actual_interpretation_markers_no_update").await,
                    );
                    sqlx::query("UPDATE hub_submissions SET capture_body=?,capture_hash=? WHERE operation_id=?")
                    .bind(forged.as_bytes()).bind(metadata.capture_digest.as_slice()).bind(operation.as_str()).execute(&mut raw).await.unwrap();
                    sqlx::query("UPDATE hub_actual_interpretation_markers SET capture_hash=? WHERE operation_id=?")
                    .bind(metadata.capture_digest.as_slice()).bind(operation.as_str()).execute(&mut raw).await.unwrap();
                }
                _ => unreachable!(),
            }
            for sql in guards {
                sqlx::query(AssertSqlSafe(sql))
                    .execute(&mut raw)
                    .await
                    .unwrap();
            }
            raw.close().await.unwrap();
            let before = crate::hub::custody::tests::material(&fixture).await;
            assert_eq!(
                db.hub_actual_review_operation(owner(), operation.clone())
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::StorageCorrupt,
                "fault {fault}"
            );
            assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
            db.close().await.unwrap();
            let db = fixture.open().await;
            assert_eq!(
                db.hub_actual_review_operation(owner(), operation)
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::StorageCorrupt,
                "reopen fault {fault}"
            );
            assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
            db.close().await.unwrap();
        }
    }
}
