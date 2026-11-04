//! New effects require the original owned fresh grant; saved pending is evidence.
use super::*;
use crate::hub::corrections::projection_native_tests::found_review_tests::posting_tests::unposted_fixture;
use harvestcircle_domain::{SafeErrorCode, hub::corrections::FoundCorrectionCommand};

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}

async fn admit(
    db: &Database,
    scope: HubSubmissionScope,
    command: FoundCorrectionCommand,
) -> admission::FreshAdmission {
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(1700),
                    HubMutation::FoundCorrection(Box::new(command)),
                    at(200),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("fresh found grant")
    };
    grant
}

#[tokio::test]
async fn complete_found_writer_late_rollback_retains_exact_pending_raw_fact_and_every_old_row() {
    let (fixture, db, scope, command) = unposted_fixture().await;
    let grant = admit(&db, scope, command.clone()).await;
    let pending = db
        .hub_submission(owner(), operation(1700))
        .await
        .unwrap()
        .unwrap();
    let material = crate::hub::custody::tests::material(&fixture).await;
    let result = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let HubMutation::FoundCorrection(command) = grant.mutation else {
                    panic!("typed found")
                };
                let original = crate::hub::corrections::found_write::apply_on(
                    tx,
                    grant.scope.actor,
                    grant.operation,
                    *command,
                    at(200),
                )
                .await?;
                assert_eq!(
                    (
                        original.original.amendment.before.value(),
                        original.original.amendment.after.value()
                    ),
                    (6, 4)
                );
                let hub = reference::authorize_on(tx, owner()).await?;
                // The actual complete writer and every full readback check succeeded.
                assert!(
                    stock::read::operation_on(tx, hub.id(), &operation(1700))
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
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
    assert_eq!(
        db.hub_submission(owner(), operation(1700))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(
        db.hub_found_correction_original(owner(), operation(1700))
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        db.hub_correction_observation_original(owner(), operation(1600))
            .await
            .unwrap()
            .operation,
        operation(1600)
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
        db.hub_submission(owner(), operation(1700))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1700),
            HubMutation::FoundCorrection(Box::new(command.clone())),
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
            operation(1701),
            HubMutation::FoundCorrection(Box::new(command)),
            at(220)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn dropped_found_grant_and_session_loss_never_renew_saved_execution() {
    for stale in [false, true] {
        let (fixture, db, scope, command) = unposted_fixture().await;
        let grant = admit(&db, scope, command.clone()).await;
        let pending = db
            .hub_submission(owner(), operation(1700))
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
            // Known stale admission may gain only a rejection, never a found root.
            assert_eq!(
                db.hub_found_correction_original(owner(), operation(1700))
                    .await
                    .unwrap(),
                None
            );
            let after = crate::hub::custody::tests::material(&fixture).await;
            let stock_rows = |rows: Vec<(String, Vec<String>)>| {
                rows.into_iter()
                    .filter(|(t, _)| t != "hub_submission_rejections")
                    .collect::<Vec<_>>()
            };
            assert_eq!(stock_rows(after), stock_rows(before));
            let mut rejected = pending.clone();
            rejected.status =
                crate::HubSubmissionStatus::Rejected(SafeErrorCode::HubPermissionDenied);
            assert_eq!(
                db.hub_submission(owner(), operation(1700))
                    .await
                    .unwrap()
                    .unwrap(),
                rejected
            );
            let mut raw = fixture.raw().await;
            let rejection: (String, String, i64) = sqlx::query_as(
                "SELECT operation_id,code,recorded_at_unix_s FROM hub_submission_rejections",
            )
            .fetch_one(&mut raw)
            .await
            .unwrap();
            assert_eq!(
                rejection,
                (
                    operation(1700).as_str().into(),
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
                    operation(1700),
                    HubMutation::FoundCorrection(Box::new(command)),
                    at(210)
                )
                .await
                .unwrap_err()
                .code(),
                SafeErrorCode::HubOutcomeUnknown
            );
            assert_eq!(
                db.hub_submission(owner(), operation(1700))
                    .await
                    .unwrap()
                    .unwrap(),
                pending
            );
            assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        }
        db.close().await.unwrap();
    }
}
