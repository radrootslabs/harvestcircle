//! Direct new-kind custody and quota proof. The capacity batch is fixture
//! construction, not a claim of separately committed admission crash recovery.
use super::*;
use crate::hub::stock::tests::other;
use harvestcircle_domain::{SafeErrorCode, hub::corrections::HUB_CORRECTION_OBSERVATION_CAPACITY};

fn input() -> CorrectionObservationCommand {
    CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::CountClassification {
            explanation_operation: HubRecordId::from_bytes([7; 16]).unwrap(),
            corrected_packs: WholePacks::from_value(0).unwrap(),
            report: None,
            basis: SourceProvenance::new(
                "Observed zero classification; no effect or review authority",
            )
            .unwrap(),
        },
    }
}
async fn active(db: &Database) -> HubSubmissionScope {
    let runtime = db.begin_hub_runtime(now()).await.unwrap();
    let generation = db
        .advance_hub_session(runtime, SessionGeneration::initial(), Some(owner()), now())
        .await
        .unwrap();
    HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    }
}

#[tokio::test]
async fn observation_denies_stale_foreign_signed_out_and_unadmitted_writers_without_mutation() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let old = active(&db).await;
    let next = db
        .advance_hub_session(old.runtime, old.generation, Some(other()), now())
        .await
        .unwrap();
    for scope in [
        old,
        HubSubmissionScope {
            generation: next,
            actor: other(),
            ..old
        },
    ] {
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.submit_hub_mutation(
                scope,
                operation(2),
                HubMutation::CorrectionObservation(Box::new(input())),
                now()
            )
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::HubPermissionDenied
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    }
    let signed_out = db
        .advance_hub_session(old.runtime, next, None, now())
        .await
        .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.submit_hub_mutation(
            HubSubmissionScope {
                generation: signed_out,
                ..old
            },
            operation(2),
            HubMutation::CorrectionObservation(Box::new(input())),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    active(&db).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let result = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                crate::hub::corrections::observe_on(tx, owner(), operation(2), input(), now()).await
            })
        })
        .await
        .map_err(map_transaction_error);
    assert_eq!(
        result.unwrap_err().code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn dropped_observation_grant_remains_exact_pending_after_reopen_and_never_renews_execution() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let scope = active(&db).await;
    let stock = db.hub_stock_page(owner(), query()).await.unwrap();
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(2),
                    HubMutation::CorrectionObservation(Box::new(input())),
                    now(),
                )
                .await
            })
        })
        .await
        .unwrap();
    assert!(matches!(decision, admission::Decision::Fresh(_)));
    drop(decision);
    let flags = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let all = admission::uncertainty_all_on(tx).await?;
                let except = admission::uncertainty_on(tx, &operation(2)).await?;
                Ok::<_, SafeError>((
                    (all.uncertain_intent, all.unresolved_actual),
                    (except.uncertain_intent, except.unresolved_actual),
                ))
            })
        })
        .await
        .unwrap();
    assert_eq!(flags, ((true, false), (false, false)));
    let pending = db
        .hub_submission(owner(), operation(2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    db.close().await.unwrap();
    let reopened = fixture.open().await;
    let scope = active(&reopened).await;
    assert_eq!(
        reopened
            .hub_submission(owner(), operation(2))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(
        reopened
            .hub_correction_observation_original(owner(), operation(2))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        reopened
            .submit_hub_mutation(
                scope,
                operation(2),
                HubMutation::CorrectionObservation(Box::new(input())),
                now()
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    assert_eq!(
        reopened
            .submit_hub_mutation(
                scope,
                operation(3),
                HubMutation::Receive(Box::new(command(&catalog))),
                now()
            )
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        reopened.hub_stock_page(owner(), query()).await.unwrap(),
        stock
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn exact_observation_body_capacity_preserves_one_pending_overflow_and_original_replay() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    setup(&db).await;
    let scope = active(&db).await;
    // Actual typed writers inside one fixture transaction; no fake cardinality,
    // altered limit, cloned archived command or fabricated terminal result.
    let (first, last) = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let mut first = None;
                let mut last = None;
                for n in 0..HUB_CORRECTION_OBSERVATION_CAPACITY {
                    let mut command = input();
                    command.previous = last
                        .as_ref()
                        .map(|r: &crate::HubCorrectionObservationReceipt| r.original.operation);
                    let decision = admission::prepare_on(
                        tx,
                        scope,
                        operation(10_000 + u32::try_from(n).unwrap()),
                        HubMutation::CorrectionObservation(Box::new(command)),
                        now(),
                    )
                    .await?;
                    let admission::Decision::Fresh(grant) = decision else {
                        panic!("new capacity fixture grant")
                    };
                    let HubMutation::CorrectionObservation(command) = grant.mutation else {
                        panic!("owned typed observation")
                    };
                    let result = crate::hub::corrections::observe_on(
                        tx,
                        grant.scope.actor,
                        grant.operation,
                        *command,
                        now(),
                    )
                    .await?;
                    if first.is_none() {
                        first = Some(result.clone());
                    }
                    last = Some(result);
                }
                Ok::<_, SafeError>((first.unwrap(), last.unwrap()))
            })
        })
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let overflow = operation(10_000 + u32::try_from(HUB_CORRECTION_OBSERVATION_CAPACITY).unwrap());
    let mut command = input();
    command.previous = Some(last.original.operation);
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            overflow.clone(),
            HubMutation::CorrectionObservation(Box::new(command.clone())),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    let pending = db
        .hub_submission(owner(), overflow.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    assert_eq!(
        db.hub_correction_observation_original(owner(), overflow.clone())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    let mut raw = fixture.raw().await;
    let counts:(i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM hub_correction_observations),(SELECT count(*) FROM hub_stock_operations WHERE kind='correction_observe'),(SELECT count(*) FROM hub_submissions WHERE kind='correction_observe'),(SELECT count(*) FROM hub_submission_rejections)").fetch_one(&mut raw).await.unwrap();
    let cap = i64::try_from(HUB_CORRECTION_OBSERVATION_CAPACITY).unwrap();
    assert_eq!(counts, (cap, cap, cap + 1, 0));
    sqlx::Connection::close(raw).await.unwrap();
    let material = crate::hub::custody::tests::material(&fixture).await;
    let next = operation(10_001 + u32::try_from(HUB_CORRECTION_OBSERVATION_CAPACITY).unwrap());
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            next.clone(),
            HubMutation::CorrectionObservation(Box::new(command.clone())),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubCapacity
    );
    assert!(db.hub_submission(owner(), next).await.unwrap().is_none());
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            overflow.clone(),
            HubMutation::CorrectionObservation(Box::new(command)),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    let crate::HubMutationResult::CorrectionObserved(replay) = db
        .submit_hub_mutation(
            scope,
            first.operation.clone(),
            HubMutation::CorrectionObservation(Box::new(input())),
            now(),
        )
        .await
        .unwrap()
    else {
        panic!("original at capacity")
    };
    assert_eq!(*replay, first);
    assert_eq!(
        db.hub_correction_observation_original(owner(), last.operation.clone())
            .await
            .unwrap(),
        last
    );
    assert_eq!(
        db.hub_submission(owner(), overflow).await.unwrap().unwrap(),
        pending
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
    db.close().await.unwrap();
}
