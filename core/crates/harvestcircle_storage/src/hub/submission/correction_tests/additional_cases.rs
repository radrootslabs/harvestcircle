//! A complete additional writer can roll back without renewing saved intent.
use super::*;
use harvestcircle_domain::{SafeErrorCode, hub::corrections::AdditionalQuantityCommand};

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}

async fn world() -> (
    Fixture,
    Database,
    HubSubmissionScope,
    AdditionalQuantityCommand,
) {
    let (fixture, db, command) = crate::hub::corrections::additional_review_fixture().await;
    let runtime = db.begin_hub_runtime(at(190)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(190),
        )
        .await
        .unwrap();
    (
        fixture,
        db,
        HubSubmissionScope {
            runtime,
            generation,
            actor: owner(),
        },
        command,
    )
}

async fn admit(
    db: &Database,
    scope: HubSubmissionScope,
    command: AdditionalQuantityCommand,
) -> admission::FreshAdmission {
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(1900),
                    HubMutation::AdditionalQuantity(Box::new(command)),
                    at(200),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("owned fresh additional grant")
    };
    grant
}

#[tokio::test]
async fn complete_additional_late_rollback_retains_every_old_row_pending_and_raw_report_after_restart()
 {
    let (fixture, db, scope, command) = world().await;
    let grant = admit(&db, scope, command.clone()).await;
    let pending = db
        .hub_submission(owner(), operation(1900))
        .await
        .unwrap()
        .unwrap();
    let report = db
        .hub_actual_report_operation(owner(), operation(1800))
        .await
        .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let result = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let HubMutation::AdditionalQuantity(command) = grant.mutation else {
                    panic!("typed additional")
                };
                let original = Box::pin(crate::hub::corrections::additional_write::apply_on(
                    tx,
                    grant.scope.actor,
                    grant.operation,
                    *command,
                    at(200),
                ))
                .await?;
                assert_eq!(
                    (
                        original.original.amendment.before.value(),
                        original.original.amendment.after.value()
                    ),
                    (2, 4)
                );
                let hub = reference::authorize_on(tx, owner()).await?;
                assert!(
                    stock::read::operation_on(tx, hub.id(), &operation(1900))
                        .await?
                        .is_some()
                );
                Err::<(), SafeError>(storage_unavailable())
            })
        })
        .await
        .map_err(map_transaction_error);
    assert_eq!(
        result.unwrap_err().code(),
        SafeErrorCode::StorageUnavailable
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    assert_eq!(
        db.hub_submission(owner(), operation(1900))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(1800))
            .await
            .unwrap(),
        report
    );
    assert_eq!(
        db.hub_additional_quantity_original(owner(), operation(1900))
            .await
            .unwrap(),
        None
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    let runtime = db.begin_hub_runtime(at(210)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(210),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::AdditionalQuantity(Box::new(command.clone())),
            at(220)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1901),
            HubMutation::AdditionalQuantity(Box::new(command)),
            at(220)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_submission(owner(), operation(1900))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(1800))
            .await
            .unwrap(),
        report
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn dropped_additional_grant_or_session_loss_cannot_renew_saved_execution() {
    for stale in [false, true] {
        let (fixture, db, scope, command) = world().await;
        let grant = admit(&db, scope, command.clone()).await;
        let pending = db
            .hub_submission(owner(), operation(1900))
            .await
            .unwrap()
            .unwrap();
        if stale {
            db.advance_hub_session(scope.runtime, scope.generation, None, at(210))
                .await
                .unwrap();
            let before = crate::hub::custody::tests::material(&fixture).await;
            assert_eq!(
                super::super::execute::execute_fresh(&db, grant, at(220))
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::HubPermissionDenied
            );
            let stock_rows = |rows: Vec<(String, Vec<String>)>| {
                rows.into_iter()
                    .filter(|(table, _)| table != "hub_submission_rejections")
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                stock_rows(crate::hub::custody::tests::material(&fixture).await),
                stock_rows(before)
            );
            let mut rejected = pending;
            rejected.status =
                crate::HubSubmissionStatus::Rejected(SafeErrorCode::HubPermissionDenied);
            assert_eq!(
                db.hub_submission(owner(), operation(1900))
                    .await
                    .unwrap()
                    .unwrap(),
                rejected
            );
            let mut raw = fixture.raw().await;
            let row: (String, String, i64) = sqlx::query_as(
                "SELECT operation_id,code,recorded_at_unix_s FROM hub_submission_rejections",
            )
            .fetch_one(&mut raw)
            .await
            .unwrap();
            assert_eq!(
                row,
                (
                    operation(1900).as_str().into(),
                    "permission_denied".into(),
                    220
                )
            );
        } else {
            drop(grant);
            let before = crate::hub::custody::tests::material(&fixture).await;
            assert_eq!(
                db.submit_hub_mutation(
                    scope,
                    operation(1900),
                    HubMutation::AdditionalQuantity(Box::new(command)),
                    at(210)
                )
                .await
                .unwrap_err()
                .code(),
                SafeErrorCode::HubOutcomeUnknown
            );
            assert_eq!(
                db.hub_submission(owner(), operation(1900))
                    .await
                    .unwrap()
                    .unwrap(),
                pending
            );
            assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        }
        assert_eq!(
            db.hub_additional_quantity_original(owner(), operation(1900))
                .await
                .unwrap(),
            None
        );
        db.close().await.unwrap();
    }
}
