//! New effects require the original owned fresh grant; saved pending is evidence.
use super::*;
use crate::hub::corrections::projection_native_tests::found_review_tests::posting_tests::unposted_fixture;
use harvestcircle_domain::{SafeErrorCode, hub::corrections::FoundCorrectionCommand};

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}

#[tokio::test]
async fn additional_comparison_cannot_exclude_a_different_pending_fact_after_reopen() {
    use crate::{HubAdditionalQuantityReviewOutcome as Outcome, HubSubmissionStatus};
    use harvestcircle_domain::hub::corrections::QuantityEffectError;
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
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(1950),
                    HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                        previous: None,
                        facts: CorrectionObservedFacts::CountClassification {
                            explanation_operation: crate::hub::stock::tests::id(99),
                            corrected_packs: WholePacks::from_value(0).unwrap(),
                            report: None,
                            basis: SourceProvenance::new(
                                "Independent unverified fact remains pending",
                            )
                            .unwrap(),
                        },
                    })),
                    at(190),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("fresh owned admission")
    };
    let pending = db
        .hub_submission(owner(), operation(1950))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, HubSubmissionStatus::Pending);
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), command.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::Blocked(QuantityEffectError::UncertainSubmission)
    );
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1950), command.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::OperationAlreadyUsed
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    drop(grant);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_submission(owner(), operation(1950))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), command, at(200))
            .await
            .unwrap()
            .outcome,
        Outcome::Blocked(QuantityEffectError::UncertainSubmission)
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
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
    let observed = db
        .hub_correction_observation_original(owner(), operation(1600))
        .await
        .unwrap();
    let harvestcircle_domain::hub::corrections::CorrectionObservedFacts::Found {
        target_operation,
        target_group,
        ..
    } = observed.original.observation.facts
    else {
        panic!("found observation")
    };
    let funding = db
        .hub_correction_target_funding(
            owner(),
            crate::HubCorrectionFundingQuery {
                target_operation: crate::hub::actuals::common::operation(target_operation).unwrap(),
                original_group: target_group,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let known = db
        .hub_correction_known_departures(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace: funding.quantity.original().trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert!(known.uncertain_intent && known.unresolved_actual);
    assert_eq!(known.departures.len(), 1);
    assert_eq!(known.departures[0].original.lines[0].packs.value(), 6);
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
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
