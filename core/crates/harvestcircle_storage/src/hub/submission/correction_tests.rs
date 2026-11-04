//! The actual new observation writer completes and reads back, then the owned
//! fixture transaction rolls back. No schema mutation or product fault hook.
use super::*;
mod additional_cases;
mod admission_cases;
mod found_cases;
use crate::hub::stock::tests::{Fixture, command, now, operation, owner, query, setup};
use harvestcircle_application::SessionGeneration;
use harvestcircle_domain::{
    UnixTimestamp,
    hub::corrections::{CorrectionObservationCommand, CorrectionObservedFacts},
    hub::reference::{SourceProvenance, WholePacks},
};

#[tokio::test]
async fn complete_observation_writer_rollback_retains_exact_pending_without_stock_or_work_change() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), now())
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let at = UnixTimestamp::from_seconds(140).unwrap();
    let runtime = db.begin_hub_runtime(at).await.unwrap();
    let generation = db
        .advance_hub_session(runtime, SessionGeneration::initial(), Some(owner()), at)
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let input = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::CountClassification {
            explanation_operation: HubRecordId::from_bytes([7; 16]).unwrap(),
            corrected_packs: WholePacks::from_value(0).unwrap(),
            report: None,
            basis: SourceProvenance::new(
                "Observed zero, factual classification review remains outstanding",
            )
            .unwrap(),
        },
    };
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(2),
                    HubMutation::CorrectionObservation(Box::new(input)),
                    UnixTimestamp::from_seconds(150).unwrap(),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("fresh observation admission")
    };
    let pending = db
        .hub_submission(owner(), operation(2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    let result = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let HubMutation::CorrectionObservation(command) = grant.mutation else {
                    panic!("typed observation")
                };
                let original = crate::hub::corrections::observe_on(
                    tx,
                    grant.scope.actor,
                    grant.operation,
                    *command,
                    UnixTimestamp::from_seconds(160).unwrap(),
                )
                .await?;
                assert_eq!(
                    original.original.observation.facts.family(),
                    "count_classification"
                );
                Err::<(), SafeError>(storage_unavailable())
            })
        })
        .await;
    assert!(result.is_err());
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(receipt)
    );
    assert_eq!(
        db.hub_submission(owner(), operation(2))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    let mut raw = fixture.raw().await;
    let (observations,roots,rejections):(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM hub_correction_observations),(SELECT count(*) FROM hub_stock_operations WHERE kind='correction_observe'),(SELECT count(*) FROM hub_submission_rejections)").fetch_one(&mut raw).await.unwrap();
    assert_eq!((observations, roots, rejections), (0, 0, 0));
    drop(raw);
    db.close().await.unwrap();
    drop(db);
    let db = fixture.open().await;
    assert_eq!(
        db.hub_submission(owner(), operation(2))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), before);
}
