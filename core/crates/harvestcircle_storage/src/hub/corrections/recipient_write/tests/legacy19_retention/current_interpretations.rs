//! Explicit current19 producer proof; historical retained profiles stay unopened.
use super::*;
use crate::hub::stock::tests::other;
use harvestcircle_domain::SafeErrorCode;
use sqlx::{AssertSqlSafe, Connection};

async fn query(db: &Database) -> crate::HubCorrectionKnownQuery {
    let actual = db
        .hub_actual_review_operation(owner(), operation(81))
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Departure(plan) = actual.original else {
        panic!("actual")
    };
    crate::HubCorrectionKnownQuery {
        trace: plan.reduction.before.trace,
        expected_stock_revision: None,
        expected_catalog_revision: None,
    }
}

#[tokio::test]
#[ignore = "Explicit current19 rich interpretation producer qualification before native20; no retained world opens"]
async fn current19_native_interpretation_keeps_zero_all_recipients_uncertainty_and_clock() {
    let mut fixture = Fixture::new();
    fixture.build = build();
    let db = fixture.open().await;
    Box::pin(drive_native19(&db)).await;
    let query = query(&db).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let evidence = db
        .hub_actual_known_interpretations(owner(), query.clone())
        .await
        .unwrap();
    assert!(!evidence.uncertain_intent && evidence.unresolved_actual);
    assert_eq!(evidence.interpretations.latest_recorded_at, at(380));
    assert_eq!(evidence.interpretations.records.len(), 1);
    let record = &evidence.interpretations.records[0];
    let view = record.view().unwrap();
    assert_eq!(
        (view.original_packs.value(), view.effective_packs.value()),
        (2, 0)
    );
    assert_eq!(record.recipient_history.len(), 2);
    assert!(view.recipients.has_unknown());
    assert_eq!(
        view.recipients.parties().collect::<Vec<_>>(),
        vec![id(3), id(4)]
    );
    assert!(view.recipients.could_match(None));
    assert!(view.recipients.could_match(Some(id(3))) && view.recipients.could_match(Some(id(4))));
    assert!(
        view.could_match_packs(None)
            && view.could_match_packs(Some(WholePacks::from_value(2).unwrap()))
    );
    assert!(view.could_match_packs(Some(WholePacks::from_value(0).unwrap())));
    assert_eq!(
        db.hub_actual_known_interpretations(other(), query.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_known_interpretations(owner(), query)
            .await
            .unwrap(),
        evidence
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
#[ignore = "Explicit current19 canonical zero-effect-root fault before native20; disposable fixture only"]
async fn current19_interpretation_cannot_hide_damaged_zero_effect_link_outside_candidate_list() {
    let mut fixture = Fixture::new();
    fixture.build = build();
    let db = fixture.open().await;
    Box::pin(drive_native19(&db)).await;
    let query = query(&db).await;
    let mut raw = fixture.raw().await;
    let trigger: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE name='hub_actual_reviews_no_update'",
    )
    .fetch_one(&mut raw)
    .await
    .unwrap();
    sqlx::query("DROP TRIGGER hub_actual_reviews_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_actual_reviews SET result_hash=zeroblob(32) WHERE operation_id=?")
        .bind(operation(LINK).as_str())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query(AssertSqlSafe(trigger))
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_known_interpretations(owner(), query)
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
