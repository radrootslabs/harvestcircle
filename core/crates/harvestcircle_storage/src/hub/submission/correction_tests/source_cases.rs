use super::*;
use crate::{HubAttributionReviewOutcome, HubSubmissionStatus};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        corrections::{AttributionError, SourceAttributionCommand},
        reference::{
            SourceAttribution, SourceCertainty, SourceCode, SourceReference, SourceReferenceKind,
        },
    },
};

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
async fn world() -> (
    Fixture,
    Database,
    HubSubmissionScope,
    SourceAttributionCommand,
) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let received = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
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
        SourceProvenance::new("Independent source interpretation, no physical permission").unwrap();
    let input = SourceAttributionCommand {
        trace: received.original.trace.id,
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_prior: None,
        source: SourceAttribution::Referenced {
            source: SourceReference {
                id: crate::hub::stock::tests::id(90),
                issuer: None,
                kind: SourceReferenceKind::Other,
                code: SourceCode::new("Fictional possible source").unwrap(),
                provenance: basis.clone(),
            },
            certainty: SourceCertainty::ReportedPossible,
        },
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
    input: SourceAttributionCommand,
) -> admission::FreshAdmission {
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(1900),
                    HubMutation::SourceAttribution(Box::new(input)),
                    at(200),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("fresh source grant")
    };
    grant
}

#[tokio::test]
async fn complete_source_writer_late_rollback_preserves_exact_pending_and_every_original_row_after_restart()
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
                let HubMutation::SourceAttribution(command) = grant.mutation else {
                    panic!("source")
                };
                let posted = Box::pin(crate::hub::corrections::source_write::apply_on(
                    tx,
                    grant.scope.actor,
                    grant.operation,
                    *command,
                    at(200),
                ))
                .await?;
                assert_eq!(posted.original.after.source, input.source);
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
        db.hub_source_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        None
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn dropped_source_grant_stays_pending_and_stale_session_cannot_execute_or_reconstruct_it() {
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
                    HubMutation::SourceAttribution(Box::new(input.clone())),
                    at(210)
                )
                .await
                .unwrap_err()
                .code(),
                SafeErrorCode::HubOutcomeUnknown
            );
            assert_eq!(
                db.hub_source_attribution_review(owner(), operation(1901), input.clone(), at(210))
                    .await
                    .unwrap()
                    .outcome,
                HubAttributionReviewOutcome::Blocked(AttributionError::UncertainSubmission)
            );
            assert_eq!(
                db.hub_source_attribution_review(owner(), operation(1900), input, at(210))
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
            db.hub_source_attribution_original(owner(), operation(1900))
                .await
                .unwrap(),
            None
        );
        let before = crate::hub::custody::tests::material(&fixture).await;
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(
            db.hub_source_attribution_original(owner(), operation(1900))
                .await
                .unwrap(),
            None
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn source_foreign_owner_stale_cuts_prior_no_change_and_nonparty_issuer_cannot_admit_a_write()
{
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
            HubMutation::SourceAttribution(Box::new(input.clone())),
            at(200)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(
        db.hub_source_attribution_original(other, operation(1900))
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
                invalid.source = SourceAttribution::Unknown;
                SafeErrorCode::HubInvalidInput
            }
            _ => {
                if let SourceAttribution::Referenced { source, .. } = &mut invalid.source {
                    source.issuer = Some(crate::hub::stock::tests::id(1));
                }
                SafeErrorCode::HubInvalidInput
            }
        };
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.submit_hub_mutation(
                scope,
                operation(1900),
                HubMutation::SourceAttribution(Box::new(invalid)),
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
