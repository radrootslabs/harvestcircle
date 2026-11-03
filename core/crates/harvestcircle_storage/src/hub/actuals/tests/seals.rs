use super::mutations::{report, review, scope};
use super::*;

async fn corrupted_seal_world(projection: bool) {
    use harvestcircle_application::hub_submission::HubMutation;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let scope = scope(&db).await;
    let crate::HubMutationResult::ActualReported(saved) = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(report())),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("report")
    };
    let command = review(&db, saved.original.id, intake.original.position.group).await;
    db.submit_hub_mutation(
        scope,
        operation(81),
        HubMutation::ActualReview(Box::new(command)),
        at(160),
    )
    .await
    .unwrap();
    let mut raw = fixture.raw().await;
    let ddl: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE name='hub_actual_reviews_no_update'",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    sqlx::query("DROP TRIGGER hub_actual_reviews_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    let fault = if projection {
        sqlx::query(
            "UPDATE hub_actual_reviews SET projection_hash=zeroblob(32) WHERE operation_id=?",
        )
    } else {
        sqlx::query("UPDATE hub_actual_reviews SET result_hash=zeroblob(32) WHERE operation_id=?")
    };
    assert_eq!(
        fault
            .bind(operation(81).as_str())
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    sqlx::query(sqlx::AssertSqlSafe(ddl.clone()))
        .execute(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_schema WHERE name='hub_actual_reviews_no_update'"
        )
        .fetch_one(&mut raw)
        .await
        .unwrap(),
        ddl
    );
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(81))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    let source = db
        .hub_actual_source(
            owner(),
            HubActualSourceQuery {
                context: query(intake.original.trace.id),
                operation: operation(1),
                group: intake.original.position.group,
            },
        )
        .await;
    if projection {
        assert_eq!(source.unwrap_err().code(), SafeErrorCode::StorageCorrupt);
    } else {
        // Separately sealed physical facts remain readable. They are not a
        // verified full original or fresh authority to change current stock.
        let source = source.unwrap();
        assert_eq!(source.known_departures.len(), 1);
        assert_eq!(source.known_departures[0].packs().unwrap(), packs(2));
        assert_eq!(
            db.hub_group(owner(), intake.original.position.group)
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
#[tokio::test]
async fn broken_full_review_seal_never_becomes_current_work_permission() {
    corrupted_seal_world(false).await;
}
#[tokio::test]
async fn broken_physical_projection_seal_never_becomes_source_or_duplicate_evidence() {
    corrupted_seal_world(true).await;
}
