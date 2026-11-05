use super::*;
use crate::hub::stock::tests::{operation, owner};
use harvestcircle_application::hub_submission::{HubMutation, HubSubmissionScope};
use harvestcircle_domain::SafeErrorCode;

#[tokio::test]
async fn physical20_cannot_admit_owned_count_effects_or_create_pending_rows() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 20);
    let (fixture, db, command) = super::super::count_review::tests::world().await;
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    let classification = CountClassificationCommand {
        observation_operation: crate::hub::actuals::common::operation_id(&operation(800)).unwrap(),
        expected_stock_revision: command.facts.expected_stock_revision,
        expected_catalog_revision: command.facts.expected_catalog_revision,
        expected_account_prior: None,
        expected_prior: None,
    };
    let before = crate::hub::custody::tests::material(&fixture).await;
    for (number, mutation) in [
        (801, HubMutation::CountExplanation(Box::new(command))),
        (
            802,
            HubMutation::CountClassification(Box::new(classification)),
        ),
    ] {
        let result = db
            .submit_hub_mutation(
                scope,
                operation(number),
                mutation,
                UnixTimestamp::from_seconds(200).unwrap(),
            )
            .await;
        assert_eq!(result.err().unwrap().code(), SafeErrorCode::HubInvalidInput);
        assert!(
            db.hub_submission(owner(), operation(number))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.hub_count_explanation_operation(owner(), operation(number))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.hub_count_classification_operation(owner(), operation(number))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    }
    db.close().await.unwrap();
}
