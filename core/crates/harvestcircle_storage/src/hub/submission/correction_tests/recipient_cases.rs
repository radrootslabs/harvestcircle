use super::*;
use crate::{HubAttributionReviewOutcome, HubSubmissionStatus};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        corrections::{AttributionError, RecipientAttributionCommand},
        departure::DepartureCommand,
        stock::Occurrence,
    },
};

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
async fn world() -> (
    Fixture,
    Database,
    HubSubmissionScope,
    RecipientAttributionCommand,
) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let paper = crate::hub::handling::departure::tests::issued_departure_fixture(&db).await;
    let current = db
        .hub_dispatch_record(owner(), paper.record.id)
        .await
        .unwrap();
    db.apply_hub_departure(
        owner(),
        operation(30),
        DepartureCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            preparation: paper.record.id,
            expected_preparation_revision: paper.record.revision,
            identification: paper.record.versions[0].id,
            occurred: Occurrence::Unknown,
            observation: SourceProvenance::new(
                "Fictional ordinary departure before recipient amendment",
            )
            .unwrap(),
        },
        at(110),
    )
    .await
    .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let runtime = db.begin_hub_runtime(at(150)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(150),
        )
        .await
        .unwrap();
    let basis =
        SourceProvenance::new("Independent recipient interpretation, no physical permission")
            .unwrap();
    let input = RecipientAttributionCommand {
        departure_operation: crate::hub::actuals::common::operation_id(&operation(30)).unwrap(),
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_prior: None,
        recipient: None,
        basis,
    };
    (
        fixture,
        db,
        HubSubmissionScope {
            runtime,
            generation,
            actor: owner(),
        },
        input,
    )
}
async fn admit(
    db: &Database,
    scope: HubSubmissionScope,
    input: RecipientAttributionCommand,
) -> admission::FreshAdmission {
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(1900),
                    HubMutation::RecipientAttribution(Box::new(input)),
                    at(200),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("fresh recipient grant")
    };
    grant
}

#[tokio::test]
async fn complete_recipient_writer_late_rollback_preserves_exact_pending_and_every_original_row_after_restart()
 {
    let (fixture, db, scope, input) = world().await;
    let grant = admit(&db, scope, input.clone()).await;
    let pending = db
        .hub_submission(owner(), operation(1900))
        .await
        .unwrap()
        .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let result = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let HubMutation::RecipientAttribution(command) = grant.mutation else {
                    panic!("recipient")
                };
                let posted = Box::pin(crate::hub::corrections::recipient_write::apply_on(
                    tx,
                    grant.scope.actor,
                    grant.operation,
                    *command,
                    at(200),
                ))
                .await?;
                assert!(matches!(
                    posted.original.after,
                    harvestcircle_domain::hub::corrections::AmendedRecipient::Unknown
                ));
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
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_submission(owner(), operation(1900)).await.unwrap(),
        Some(pending)
    );
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        None
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn dropped_recipient_grant_stays_pending_and_stale_session_cannot_execute_or_reconstruct_it()
{
    for stale in [false, true] {
        let (fixture, db, scope, input) = world().await;
        let grant = admit(&db, scope, input.clone()).await;
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
            let rejection_rows = |rows: &[(String, Vec<String>)]| {
                rows.iter()
                    .find(|(name, _)| name == "hub_submission_rejections")
                    .unwrap()
                    .1
                    .clone()
            };
            assert!(rejection_rows(&before).is_empty());
            assert_eq!(
                super::super::execute::execute_fresh(&db, grant, at(220))
                    .await
                    .unwrap_err()
                    .code(),
                SafeErrorCode::HubPermissionDenied
            );
            let effects = |rows: Vec<(String, Vec<String>)>| {
                rows.into_iter()
                    .filter(|(name, _)| name != "hub_submission_rejections")
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                effects(crate::hub::custody::tests::material(&fixture).await),
                effects(before)
            );
            // The sole permitted table delta is this exact rejection, not an
            // arbitrary change hidden by the material comparison above.
            use sqlx::Connection;
            let mut raw = fixture.raw().await;
            let rows: Vec<(String,String,i64)> = sqlx::query_as("SELECT operation_id,code,recorded_at_unix_s FROM hub_submission_rejections ORDER BY operation_id").fetch_all(&mut raw).await.unwrap();
            assert_eq!(
                rows,
                vec![(
                    operation(1900).as_str().into(),
                    "permission_denied".into(),
                    220
                )]
            );
            raw.close().await.unwrap();
            let mut rejected = pending;
            rejected.status = HubSubmissionStatus::Rejected(SafeErrorCode::HubPermissionDenied);
            assert_eq!(
                db.hub_submission(owner(), operation(1900)).await.unwrap(),
                Some(rejected)
            );
        } else {
            drop(grant);
            let before = crate::hub::custody::tests::material(&fixture).await;
            assert_eq!(
                db.submit_hub_mutation(
                    scope,
                    operation(1900),
                    HubMutation::RecipientAttribution(Box::new(input.clone())),
                    at(210)
                )
                .await
                .unwrap_err()
                .code(),
                SafeErrorCode::HubOutcomeUnknown
            );
            assert_eq!(
                db.hub_recipient_attribution_review(
                    owner(),
                    operation(1901),
                    input.clone(),
                    at(210)
                )
                .await
                .unwrap()
                .outcome,
                HubAttributionReviewOutcome::Blocked(AttributionError::UncertainSubmission)
            );
            assert_eq!(
                db.hub_recipient_attribution_review(owner(), operation(1900), input, at(210))
                    .await
                    .unwrap()
                    .outcome,
                HubAttributionReviewOutcome::OperationAlreadyUsed
            );
            assert_eq!(
                db.hub_submission(owner(), operation(1900)).await.unwrap(),
                Some(pending)
            );
            assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        }
        assert_eq!(
            db.hub_recipient_attribution_original(owner(), operation(1900))
                .await
                .unwrap(),
            None
        );
        let before = crate::hub::custody::tests::material(&fixture).await;
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(
            db.hub_recipient_attribution_original(owner(), operation(1900))
                .await
                .unwrap(),
            None
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn recipient_foreign_owner_stale_cuts_prior_no_change_and_nonparty_cannot_admit_a_write() {
    let (fixture, db, scope, input) = world().await;
    let other = crate::hub::stock::tests::other();
    let generation = db
        .advance_hub_session(scope.runtime, scope.generation, Some(other), at(160))
        .await
        .unwrap();
    let foreign = HubSubmissionScope {
        generation,
        actor: other,
        ..scope
    };
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.submit_hub_mutation(
            foreign,
            operation(1900),
            HubMutation::RecipientAttribution(Box::new(input.clone())),
            at(200)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.hub_recipient_attribution_original(other, operation(1900))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    let generation = db
        .advance_hub_session(scope.runtime, generation, Some(owner()), at(170))
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        generation,
        ..scope
    };
    for fault in 0..5 {
        let mut invalid = input.clone();
        let expected = match fault {
            0 => {
                invalid.expected_stock_revision = HubRevision::INITIAL;
                SafeErrorCode::HubConflict
            }
            1 => {
                invalid.expected_catalog_revision = HubRevision::INITIAL;
                SafeErrorCode::HubConflict
            }
            2 => {
                invalid.expected_prior = Some(crate::hub::stock::tests::id(89));
                SafeErrorCode::HubConflict
            }
            3 => {
                invalid.recipient = Some(crate::hub::stock::tests::id(8));
                SafeErrorCode::HubInvalidInput
            }
            _ => {
                invalid.recipient = Some(crate::hub::stock::tests::id(1));
                SafeErrorCode::HubInvalidInput
            }
        };
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.submit_hub_mutation(
                scope,
                operation(1900),
                HubMutation::RecipientAttribution(Box::new(invalid)),
                at(200)
            )
            .await
            .unwrap_err()
            .code(),
            expected,
            "fault {fault}"
        );
        assert_eq!(
            db.hub_submission(owner(), operation(1900)).await.unwrap(),
            None
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    }
    db.close().await.unwrap();
}
