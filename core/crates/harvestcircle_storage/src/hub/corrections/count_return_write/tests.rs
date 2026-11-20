//! Historical physical20 boundary and separate current native fixtures.
pub(in crate::hub) mod native22;
use super::*;
use crate::hub::stock::tests::{operation, owner};
use harvestcircle_application::hub_submission::{HubMutation, HubSubmissionScope};
use harvestcircle_domain::{SafeErrorCode, hub::reference::SourceProvenance};

#[tokio::test]
#[ignore = "Historical published20 producer proof retained; current native22 fixtures own new behavior"]
async fn physical20_rejects_owned_and_borrowed_association_before_any_admission_or_effect() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 20);
    let (fixture, db, query) = super::super::count_review::tests::world().await;
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    let id = |n| crate::hub::actuals::common::operation_id(&operation(n)).unwrap();
    let evidence =
        || SourceProvenance::new("Explicit fictional whole-intake association evidence").unwrap();
    let command = CountReturnAssociationCommand {
        return_operation: id(900),
        explanation_operation: id(901),
        expected_stock_revision: query.facts.expected_stock_revision,
        expected_catalog_revision: query.facts.expected_catalog_revision,
        expected_account_prior: Some(id(901)),
        expected_returns_prior: None,
        source_operation: query.source_operation,
        source_group: query.source_group,
        source_evidence: evidence(),
        quantity_evidence: evidence(),
        timing_evidence: evidence(),
    };
    let before = crate::hub::custody::tests::material(&fixture).await;
    let error = db
        .submit_hub_mutation(
            scope,
            operation(902),
            HubMutation::CountReturnAssociation(Box::new(command.clone())),
            UnixTimestamp::from_seconds(200).unwrap(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), SafeErrorCode::HubInvalidInput);
    let error = db
        .capture_hub_submission(
            owner(),
            scope,
            operation(903),
            HubMutation::CountReturnAssociation(Box::new(command)),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), SafeErrorCode::HubInvalidInput);
    for n in [902, 903] {
        assert!(
            db.hub_submission(owner(), operation(n))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.hub_count_return_association_operation(owner(), operation(n))
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
