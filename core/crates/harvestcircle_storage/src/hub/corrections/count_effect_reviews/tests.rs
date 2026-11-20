use super::*;
use crate::hub::{
    corrections::count_return_write::tests::native22::association_world,
    stock::tests::{id, operation, other, owner, query},
};
use harvestcircle_application::hub_submission::{HubMutation, HubSubmissionScope};
use harvestcircle_domain::hub::{
    corrections::{CorrectionObservationCommand, CorrectionObservedFacts},
    movement::PhysicalIdentification,
    reference::{HubLabel, SourceAttribution, SourceProvenance, WholePacks},
    returns::ReturnCommand,
    stock::Occurrence,
};
use std::{future::Future, pin::Pin};
fn at(value: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(value).unwrap()
}
fn own(value: u32) -> HubRecordId {
    crate::hub::actuals::common::operation_id(&operation(value)).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Independent fictional evidence, never permission to execute").unwrap()
}

#[tokio::test]
async fn classification_review_is_read_only_and_matches_fresh_native_write_and_cold_original() {
    qualify_classification_review(false).await;
}

#[tokio::test]
async fn classification_review_keeps_real_pending_admission_blocked_across_cold_reopen() {
    qualify_classification_review(true).await;
}

async fn retain_pending_claim(db: &Database, scope: HubSubmissionScope, target: HubRecordId) {
    let error = db
        .submit_hub_mutation(
            scope,
            operation(1980),
            HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                previous: None,
                facts: CorrectionObservedFacts::CountClassification {
                    explanation_operation: target,
                    corrected_packs: WholePacks::from_value(0).unwrap(),
                    report: None,
                    basis: note(),
                },
            })),
            at(199),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.code(),
        harvestcircle_domain::SafeErrorCode::HubOutcomeUnknown
    );
    assert_eq!(
        db.hub_submission(owner(), operation(1980))
            .await
            .unwrap()
            .unwrap()
            .status,
        crate::HubSubmissionStatus::Pending
    );
    // No missing-original lookup clears this admission or authorizes execution.
}

fn qualify_classification_review(pending_case: bool) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let (fixture, db, scope, explained) =
            crate::hub::corrections::count_review::tests::explained_world().await;
        let crate::HubMutationResult::CorrectionObserved(observed) = db
            .submit_hub_mutation(
                scope,
                operation(810),
                HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                    previous: None,
                    facts: CorrectionObservedFacts::CountClassification {
                        explanation_operation: explained.original.operation,
                        corrected_packs: WholePacks::from_value(0).unwrap(),
                        report: None,
                        basis: note(),
                    },
                })),
                at(210),
            )
            .await
            .unwrap()
        else {
            panic!("ordinary recorded classification observation")
        };
        let page = db.hub_stock_page(owner(), query()).await.unwrap();
        let command = CountClassificationCommand {
            observation_operation: observed.original.operation,
            expected_stock_revision: page.revision,
            expected_catalog_revision: page.catalog_revision,
            expected_account_prior: Some(explained.original.operation),
            expected_prior: None,
        };
        let before = crate::hub::custody::tests::material_digest(&fixture).await;
        let review = db
            .hub_count_classification_review(owner(), operation(811), command.clone(), at(220))
            .await
            .unwrap();
        assert!(!review.uncertain_intent);
        assert!(!review.unresolved_return_association);
        assert_eq!(
            db.hub_count_classification_review(owner(), operation(810), command.clone(), at(220),)
                .await
                .unwrap()
                .outcome,
            HubCountClassificationReviewOutcome::OperationAlreadyUsed
        );
        assert_eq!(
            db.hub_count_classification_review(other(), operation(811), command.clone(), at(220),)
                .await
                .unwrap_err()
                .code(),
            harvestcircle_domain::SafeErrorCode::HubPermissionDenied
        );
        let mut stale = command.clone();
        stale.expected_stock_revision = explained.original.explanation.stock_revision;
        assert_eq!(
            db.hub_count_classification_review(owner(), operation(811), stale, at(220),)
                .await
                .unwrap_err()
                .code(),
            harvestcircle_domain::SafeErrorCode::HubConflict
        );
        assert!(
            db.hub_submission(owner(), operation(811))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            crate::hub::custody::tests::material_digest(&fixture).await,
            before
        );
        if pending_case {
            assert!(matches!(
                &review.outcome,
                HubCountClassificationReviewOutcome::Candidate(_)
            ));
            retain_pending_claim(&db, scope, explained.original.operation).await;
            let pending = crate::hub::custody::tests::material_digest(&fixture).await;
            let blocked = db
                .hub_count_classification_review(owner(), operation(811), command.clone(), at(230))
                .await
                .unwrap();
            assert!(blocked.uncertain_intent);
            assert_eq!(
                blocked.outcome,
                HubCountClassificationReviewOutcome::Blocked(
                    QuantityEffectError::UncertainSubmission
                )
            );
            assert_eq!(
                db.hub_count_classification_review(
                    owner(),
                    operation(1980),
                    command.clone(),
                    at(230)
                )
                .await
                .unwrap()
                .outcome,
                HubCountClassificationReviewOutcome::OperationAlreadyUsed
            );
            assert!(
                db.hub_submission(owner(), operation(811))
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                crate::hub::custody::tests::material_digest(&fixture).await,
                pending
            );
            db.close().await.unwrap();
            let db = fixture.open().await;
            assert_eq!(
                db.hub_count_classification_review(owner(), operation(811), command, at(230))
                    .await
                    .unwrap(),
                blocked
            );
            assert_eq!(
                db.hub_submission(owner(), operation(1980))
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                crate::HubSubmissionStatus::Pending
            );
            assert_eq!(
                crate::hub::custody::tests::material_digest(&fixture).await,
                pending
            );
            db.close().await.unwrap();
            return;
        }
        let HubCountClassificationReviewOutcome::Candidate(plan) = review.outcome else {
            panic!("complete native classification comparison")
        };
        let crate::HubMutationResult::CountClassified(posted) = db
            .submit_hub_mutation(
                scope,
                operation(811),
                HubMutation::CountClassification(Box::new(command)),
                at(220),
            )
            .await
            .unwrap()
        else {
            panic!("fresh classification admission")
        };
        assert_eq!(posted.original, *plan);
        assert_eq!(
            db.hub_stock_page(owner(), query()).await.unwrap().rows,
            page.rows
        );
        let after = crate::hub::custody::tests::material_digest(&fixture).await;
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(
            db.hub_count_classification_operation(owner(), operation(811))
                .await
                .unwrap(),
            Some(*posted)
        );
        assert_eq!(
            db.hub_count_explanation_operation(owner(), operation(801))
                .await
                .unwrap(),
            Some(*explained)
        );
        assert_eq!(
            crate::hub::custody::tests::material_digest(&fixture).await,
            after
        );
        db.close().await.unwrap();
    })
}

#[tokio::test]
async fn return_association_review_preserves_custody_and_matches_fresh_write_and_cold_original() {
    qualify_return_review(false).await;
}

#[tokio::test]
async fn return_association_review_keeps_real_pending_admission_blocked_across_cold_reopen() {
    qualify_return_review(true).await;
}

fn qualify_return_review(pending_case: bool) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let world = association_world().await;
        let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
        let crate::HubMutationResult::Returned(returned) = world
            .db
            .submit_hub_mutation(
                world.scope,
                operation(950),
                HubMutation::Return(Box::new(ReturnCommand {
                    expected_stock_revision: page.revision,
                    expected_catalog_revision: page.catalog_revision,
                    variant: id(2),
                    location: id(6),
                    returned_by: None,
                    source: SourceAttribution::Unknown,
                    packs: WholePacks::from_value(1).unwrap(),
                    identification: PhysicalIdentification {
                        label: HubLabel::new("Second independently counted restricted return")
                            .unwrap(),
                        observation: note(),
                    },
                    planned_location: None,
                    departure: None,
                    occurred: Occurrence::Known(at(90)),
                    observation: note(),
                })),
                at(270),
            )
            .await
            .unwrap()
        else {
            panic!("ordinary independent returned intake")
        };
        let page = world.db.hub_stock_page(owner(), query()).await.unwrap();
        let command = CountReturnAssociationCommand {
            return_operation: own(950),
            explanation_operation: own(905),
            expected_stock_revision: page.revision,
            expected_catalog_revision: page.catalog_revision,
            // Association history has a separate head from the E/C account.
            expected_account_prior: world.command.expected_account_prior,
            expected_returns_prior: Some(world.associated.original.association.operation),
            source_operation: world.command.source_operation,
            source_group: world.command.source_group,
            source_evidence: note(),
            quantity_evidence: note(),
            timing_evidence: note(),
        };
        let before = crate::hub::custody::tests::material_digest(&world.fixture).await;
        let review = world
            .db
            .hub_count_return_association_review(owner(), operation(951), command.clone(), at(280))
            .await
            .unwrap();
        assert!(!review.uncertain_intent);
        assert_eq!(
            world
                .db
                .hub_count_return_association_review(
                    owner(),
                    operation(906),
                    command.clone(),
                    at(280),
                )
                .await
                .unwrap()
                .outcome,
            HubCountReturnAssociationReviewOutcome::OperationAlreadyUsed
        );
        assert_eq!(
            world
                .db
                .hub_count_return_association_review(
                    other(),
                    operation(951),
                    command.clone(),
                    at(280),
                )
                .await
                .unwrap_err()
                .code(),
            harvestcircle_domain::SafeErrorCode::HubPermissionDenied
        );
        let mut wrong = command.clone();
        wrong.expected_returns_prior = None;
        assert_eq!(
            world
                .db
                .hub_count_return_association_review(owner(), operation(951), wrong, at(280),)
                .await
                .unwrap()
                .outcome,
            HubCountReturnAssociationReviewOutcome::Blocked(
                CountReturnAssociationError::StaleRevision
            )
        );
        assert!(
            world
                .db
                .hub_submission(owner(), operation(951))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            crate::hub::custody::tests::material_digest(&world.fixture).await,
            before
        );
        if pending_case {
            assert!(matches!(
                &review.outcome,
                HubCountReturnAssociationReviewOutcome::Candidate(_)
            ));
            retain_pending_claim(&world.db, world.scope, own(905)).await;
            let pending = crate::hub::custody::tests::material_digest(&world.fixture).await;
            let blocked = world
                .db
                .hub_count_return_association_review(
                    owner(),
                    operation(951),
                    command.clone(),
                    at(280),
                )
                .await
                .unwrap();
            assert!(blocked.uncertain_intent);
            assert_eq!(
                blocked.outcome,
                HubCountReturnAssociationReviewOutcome::Blocked(
                    CountReturnAssociationError::UncertainSubmission
                )
            );
            assert_eq!(
                world
                    .db
                    .hub_count_return_association_review(
                        owner(),
                        operation(1980),
                        command.clone(),
                        at(280)
                    )
                    .await
                    .unwrap()
                    .outcome,
                HubCountReturnAssociationReviewOutcome::OperationAlreadyUsed
            );
            assert!(
                world
                    .db
                    .hub_submission(owner(), operation(951))
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                crate::hub::custody::tests::material_digest(&world.fixture).await,
                pending
            );
            world.db.close().await.unwrap();
            let db = world.fixture.open().await;
            assert_eq!(
                db.hub_count_return_association_review(owner(), operation(951), command, at(280))
                    .await
                    .unwrap(),
                blocked
            );
            assert_eq!(
                db.hub_submission(owner(), operation(1980))
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                crate::HubSubmissionStatus::Pending
            );
            assert_eq!(
                crate::hub::custody::tests::material_digest(&world.fixture).await,
                pending
            );
            db.close().await.unwrap();
            return;
        }
        let HubCountReturnAssociationReviewOutcome::Candidate(plan) = review.outcome else {
            panic!("complete native Return comparison")
        };
        let crate::HubMutationResult::CountReturnAssociated(posted) = world
            .db
            .submit_hub_mutation(
                world.scope,
                operation(951),
                HubMutation::CountReturnAssociation(Box::new(command)),
                at(280),
            )
            .await
            .unwrap()
        else {
            panic!("fresh association admission")
        };
        assert_eq!(posted.original, *plan);
        assert_eq!(
            world
                .db
                .hub_stock_page(owner(), query())
                .await
                .unwrap()
                .rows,
            page.rows
        );
        assert_eq!(
            world
                .db
                .hub_return_operation(owner(), operation(950))
                .await
                .unwrap(),
            Some(*returned)
        );
        let after = crate::hub::custody::tests::material_digest(&world.fixture).await;
        world.db.close().await.unwrap();
        let db = world.fixture.open().await;
        assert_eq!(
            db.hub_count_return_association_operation(owner(), operation(951))
                .await
                .unwrap(),
            Some(*posted)
        );
        assert_eq!(
            db.hub_count_return_association_operation(owner(), operation(906))
                .await
                .unwrap(),
            Some(*world.associated)
        );
        assert_eq!(
            crate::hub::custody::tests::material_digest(&world.fixture).await,
            after
        );
        db.close().await.unwrap();
    })
}
