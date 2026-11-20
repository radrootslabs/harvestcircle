//! Header attribution cannot replace full native capture authentication.
use super::*;

#[tokio::test]
async fn damaged_admission_identity_fails_before_trace_selection_and_large_body_stays_untrusted() {
    for fault in 0..5 {
        let (fixture, db, a, b, before, after) = source_world().await;
        db.close().await.unwrap();
        let mut raw = fixture.raw().await;
        sqlx::query("DROP TRIGGER hub_submissions_no_update")
            .execute(&mut raw)
            .await
            .unwrap();
        match fault {
            0 => {
                sqlx::query("UPDATE hub_submissions SET input_hash=? WHERE operation_id=?")
                    .bind([99_u8; 32].as_slice())
                    .bind(operation(1000).as_str())
                    .execute(&mut raw)
                    .await
                    .unwrap();
            }
            1 => {
                sqlx::query("UPDATE hub_submissions SET domain_intent_hash=? WHERE operation_id=?")
                    .bind([99_u8; 32].as_slice())
                    .bind(operation(1000).as_str())
                    .execute(&mut raw)
                    .await
                    .unwrap();
            }
            2 => {
                sqlx::query("UPDATE hub_submissions SET actor_public_key=? WHERE operation_id=?")
                    .bind(crate::hub::stock::tests::other().as_bytes().as_slice())
                    .bind(operation(1000).as_str())
                    .execute(&mut raw)
                    .await
                    .unwrap();
            }
            3 => {
                sqlx::query(
                    "UPDATE hub_submissions SET submitted_at_unix_s=201 WHERE operation_id=?",
                )
                .bind(operation(1000).as_str())
                .execute(&mut raw)
                .await
                .unwrap();
            }
            4 => {
                sqlx::query("UPDATE hub_submissions SET capture_body=zeroblob(16777216) WHERE operation_id=?")
                .bind(operation(1000).as_str()).execute(&mut raw).await.unwrap();
            }
            _ => unreachable!(),
        }
        let trigger = include_str!("../../../submission/schema-v16.sql")
            .split(";\n")
            .find(|sql| {
                sql.trim()
                    .starts_with("CREATE TRIGGER hub_submissions_no_update ")
            })
            .unwrap();
        sqlx::raw_sql(trigger).execute(&mut raw).await.unwrap();
        raw.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(count(&db, a, before).await.unwrap(), 0);
        if fault == 4 {
            // A lower inventory does not fetch/hash this variable body or call
            // it authenticated. Full original recovery must independently fail.
            assert_eq!(count(&db, a, after).await.unwrap(), 1);
            assert_eq!(count(&db, b, after).await.unwrap(), 0);
        } else {
            for trace in [a, b] {
                assert_eq!(
                    count(&db, trace, after).await.unwrap_err().code(),
                    SafeErrorCode::StorageCorrupt
                );
            }
        }
        assert_eq!(
            db.hub_source_attribution_original(owner(), operation(1000))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        db.close().await.unwrap();
    }
}
