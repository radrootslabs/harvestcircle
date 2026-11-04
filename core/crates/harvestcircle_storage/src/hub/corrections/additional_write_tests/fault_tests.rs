use super::*;
use harvestcircle_domain::SafeErrorCode;
use sqlx::{AssertSqlSafe, Connection};

#[tokio::test]
async fn persisted_additional_root_capture_admission_projection_event_and_closure_faults_reject_without_repair()
 {
    for (table, update) in [
        (
            "hub_corrections",
            "UPDATE hub_corrections SET result_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_correction_captures",
            "UPDATE hub_correction_captures SET scope_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_submissions",
            "UPDATE hub_submissions SET capture_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_correction_physical_debits",
            "UPDATE hub_correction_physical_debits SET projection_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_correction_quantity_changes",
            "UPDATE hub_correction_quantity_changes SET projection_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_stock_events",
            "UPDATE hub_stock_events SET packs=packs+1 WHERE operation_id=?",
        ),
        (
            "hub_actual_report_reviews",
            "UPDATE hub_actual_report_reviews SET review_operation_id=NULL WHERE review_operation_id=?",
        ),
        (
            "hub_correction_captures",
            "DELETE FROM hub_correction_captures WHERE operation_id=?",
        ),
    ] {
        let (fixture, db, command) = super::super::additional_review::tests::actual_world().await;
        let scope = scope(&db).await;
        let crate::HubMutationResult::AdditionalCorrected(receipt) = db
            .submit_hub_mutation(
                scope,
                operation(1900),
                HubMutation::AdditionalQuantity(Box::new(command)),
                at(200),
            )
            .await
            .unwrap()
        else {
            panic!("legal post")
        };
        let mut raw = fixture.raw().await;
        assert!(
            sqlx::query(update)
                .bind(receipt.operation.as_str())
                .execute(&mut raw)
                .await
                .is_err(),
            "immutable {table}"
        );
        let triggers: Vec<(String, String)> = sqlx::query_as(
            "SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name=? ORDER BY name",
        )
        .bind(table)
        .fetch_all(&mut raw)
        .await
        .unwrap();
        assert!(!triggers.is_empty());
        for (name, _) in &triggers {
            sqlx::query(AssertSqlSafe(format!(
                "DROP TRIGGER \"{}\"",
                name.replace('"', "\"\"")
            )))
            .execute(&mut raw)
            .await
            .unwrap();
        }
        assert_eq!(
            sqlx::query(update)
                .bind(receipt.operation.as_str())
                .execute(&mut raw)
                .await
                .unwrap()
                .rows_affected(),
            1
        );
        for (_, ddl) in triggers {
            sqlx::query(AssertSqlSafe(ddl))
                .execute(&mut raw)
                .await
                .unwrap();
        }
        raw.close().await.unwrap();
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.hub_additional_quantity_original(owner(), receipt.operation.clone())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "original {table}"
        );
        assert_eq!(
            db.hub_stock_page(owner(), crate::hub::stock::tests::query())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "current {table}"
        );
        assert_eq!(
            crate::hub::custody::tests::material(&fixture).await,
            before,
            "no repair {table}"
        );
        db.close().await.unwrap();
    }
}
