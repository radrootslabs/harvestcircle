use super::*;
use harvestcircle_domain::{SafeErrorCode, hub::corrections::CountClassificationCommand};

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}

#[tokio::test]
async fn count_descriptive_capture_keeps_selected_catalog_and_never_admits_a_mutation() {
    let (fixture, db, command) = crate::hub::corrections::count_review::tests::world().await;
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    let before = crate::hub::custody::tests::material(&fixture).await;
    let input = command.clone();
    let archived = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                capture::capture_on(
                    tx,
                    owner(),
                    scope,
                    &operation(5),
                    HubSubmittedRequest::CountExplanation(&input),
                )
                .await
            })
        })
        .await
        .map_err(map_transaction_error)
        .unwrap();
    let encoded: serde_json::Value = serde_json::from_slice(archived.as_bytes()).unwrap();
    let rows = encoded["material"].as_array().unwrap();
    let count = &rows
        .iter()
        .find_map(|row| row.get("count_explanation"))
        .unwrap();
    assert_eq!(count["prior_report_review"], serde_json::Value::Null);
    assert_eq!(count["uncertain_submissions"], false);
    assert_eq!(count["references"].as_array().unwrap().len(), 3);
    assert!(
        rows.iter()
            .filter_map(|row| row.get("reference"))
            .any(|row| row["id"] == serde_json::to_value(command.facts.recipient).unwrap())
    );
    assert!(
        db.hub_submission(owner(), operation(5))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);

    for foreign in [false, true] {
        let mut input = command.clone();
        if !foreign {
            input.facts.expected_stock_revision =
                HubRevision::from_value(input.facts.expected_stock_revision.value() + 1).unwrap();
        }
        let actor = if foreign {
            crate::hub::stock::tests::other()
        } else {
            owner()
        };
        let result = db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    capture::capture_on(
                        tx,
                        actor,
                        scope,
                        &operation(5),
                        HubSubmittedRequest::CountExplanation(&input),
                    )
                    .await
                })
            })
            .await
            .map_err(map_transaction_error)
            .unwrap_err();
        assert_eq!(
            result.code(),
            if foreign {
                SafeErrorCode::HubPermissionDenied
            } else {
                SafeErrorCode::HubConflict
            }
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    }
    let input = CountClassificationCommand {
        observation_operation: HubRecordId::from_bytes([88; 16]).unwrap(),
        expected_stock_revision: command.facts.expected_stock_revision,
        expected_catalog_revision: command.facts.expected_catalog_revision,
        expected_account_prior: None,
        expected_prior: None,
    };
    let rejected = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                capture::capture_on(
                    tx,
                    owner(),
                    scope,
                    &operation(5),
                    HubSubmittedRequest::CountClassification(&input),
                )
                .await
            })
        })
        .await
        .map_err(map_transaction_error)
        .unwrap_err();
    assert_eq!(rejected.code(), SafeErrorCode::HubInvalidInput);
    assert!(
        db.hub_submission(owner(), operation(5))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn count_capture_cannot_bypass_a_dropped_fresh_admission_or_reuse_its_identity() {
    let (fixture, db, command) = crate::hub::corrections::count_review::tests::world().await;
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    let review = db
        .hub_count_explanation_review(owner(), operation(5), command.clone(), at(150))
        .await
        .unwrap();
    let crate::hub::corrections::count_review::HubCountExplanationReviewOutcome::Candidate(plan) =
        review.outcome
    else {
        panic!("candidate")
    };
    let input = plan.report.report;
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(20),
                    HubMutation::ActualReport(Box::new(input)),
                    at(150),
                )
                .await
            })
        })
        .await
        .map_err(map_transaction_error)
        .unwrap();
    drop(decision);
    let pending = db
        .hub_submission(owner(), operation(20))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    let before = crate::hub::custody::tests::material(&fixture).await;
    for proposed in [operation(5), operation(20)] {
        let input = command.clone();
        let rejected = db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    capture::capture_on(
                        tx,
                        owner(),
                        scope,
                        &proposed,
                        HubSubmittedRequest::CountExplanation(&input),
                    )
                    .await
                })
            })
            .await
            .map_err(map_transaction_error)
            .unwrap_err();
        assert_eq!(rejected.code(), SafeErrorCode::HubConflict);
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    }
    assert_eq!(
        db.hub_submission(owner(), operation(20))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert!(
        db.hub_submission(owner(), operation(5))
            .await
            .unwrap()
            .is_none()
    );
    db.close().await.unwrap();
}
