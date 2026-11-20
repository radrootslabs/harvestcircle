//! Genuine ordinary Count E/C writer qualification; no final-row seeding.
//! One explanation plus classifications fills the shared correction budget.
use super::*;
use crate::hub::custody::tests::{material_digest, material_digest_for};

async fn physical_digest(fixture: &Fixture) -> [u8; 32] {
    material_digest_for(fixture, is_physical_table).await
}

#[tokio::test]
#[ignore = "Explicit default-stack native classification scaling precursor"]
async fn ordinary_native_count_classification_thirty_two_rows_reopen_cold() {
    qualify(32, false).await;
}

#[tokio::test]
#[ignore = "Explicit default-stack longer native classification prefix and cold recovery"]
async fn ordinary_native_count_classification_128_rows_reopen_cold() {
    qualify(128, false).await;
}

#[tokio::test]
#[ignore = "Explicit full native4096/4097 shared-account application-writer boundary"]
async fn ordinary_native_count_classification_4096_rows_rejects_growth_and_reopens_cold() {
    qualify(4096, true).await;
}

async fn qualify(rows: u32, full: bool) {
    let (fixture, db, scope, explanation) =
        super::super::super::super::count_review::tests::explained_world().await;
    let before = physical_digest(&fixture).await;
    let mut account_prior = id(801);
    let mut target_prior = None;
    let mut last = None;
    let mut last_input = None;
    for row in 1..rows {
        let (input, time) = observe(&db, scope, row, account_prior, target_prior).await;
        let crate::HubMutationResult::CountClassified(receipt) = Box::pin(db.submit_hub_mutation(
            scope,
            operation(220001 + row * 2),
            HubMutation::CountClassification(Box::new(input.clone())),
            at(time + 1),
        ))
        .await
        .unwrap() else {
            panic!("ordinary classification row{row}")
        };
        account_prior = receipt.original.amendment.operation;
        target_prior = Some(account_prior);
        last_input = Some(input);
        last = Some(receipt);
        if (row + 1) % 16 == 0 {
            eprintln!(
                "ordinary native Count E/C committed{} correction rows",
                row + 1
            );
        }
    }
    let last = last.unwrap();
    let trace = explanation.original.known_event.trace;
    let mut overflow = None;
    if full {
        let (input, time) = observe(&db, scope, rows, account_prior, target_prior).await;
        let stable = material_digest(&fixture).await;
        assert_eq!(
            Box::pin(db.submit_hub_mutation(
                scope,
                operation(220001 + rows * 2),
                HubMutation::CountClassification(Box::new(input)),
                at(time + 1)
            ))
            .await
            .unwrap_err()
            .code(),
            SafeErrorCode::HubCapacity
        );
        assert!(
            db.hub_submission(owner(), operation(220001 + rows * 2))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.hub_count_classification_operation(owner(), operation(220001 + rows * 2))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(material_digest(&fixture).await, stable);
        let raw = db
            .hub_correction_observation_original(owner(), operation(220000 + rows * 2))
            .await
            .unwrap();
        assert_eq!(raw.original.operation, id(220000 + rows * 2));
        overflow = Some(raw);
    }
    let stable = material_digest(&fixture).await;
    let crate::HubMutationResult::CountClassified(replay) = Box::pin(db.submit_hub_mutation(
        scope,
        last.operation.clone(),
        HubMutation::CountClassification(Box::new(last_input.unwrap())),
        at(30000),
    ))
    .await
    .unwrap() else {
        panic!("exact recorded classification replay")
    };
    assert_eq!(replay, last);
    assert_eq!(material_digest(&fixture).await, stable);
    assert_eq!(physical_digest(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    let stable = material_digest(&fixture).await;
    assert_eq!(
        db.hub_count_classification_operation(owner(), last.operation.clone())
            .await
            .unwrap()
            .as_ref(),
        Some(last.as_ref())
    );
    assert_eq!(
        db.hub_count_explanation_operation(owner(), operation(801))
            .await
            .unwrap()
            .as_ref(),
        Some(explanation.as_ref())
    );
    let current = db
        .hub_trace_snapshot(
            owner(),
            HubTraceSnapshotQuery {
                trace,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let inventory = current.count_interpretations.as_ref().unwrap();
    assert_eq!(inventory.facts().records.len(), 1);
    let account = &inventory.facts().records[0]
        .count_explanation
        .as_ref()
        .unwrap()
        .account;
    assert_eq!(account.explanations().len(), 1);
    assert_eq!(account.amendments().len(), rows as usize - 1);
    assert_eq!(account.explained().value(), 0);
    assert_eq!(account.unexplained().value(), 2);
    let balance = current.balance().unwrap();
    assert_eq!(
        (
            balance.original.value(),
            balance.on_hand.value(),
            balance.gross_counted_loss.value(),
            balance.counted_loss.value(),
            balance.actual_departed.value()
        ),
        (20, 18, 2, 2, 0)
    );
    if let Some(raw) = overflow {
        assert_eq!(
            db.hub_correction_observation_original(owner(), operation(220000 + rows * 2))
                .await
                .unwrap(),
            raw
        );
    }
    assert_eq!(material_digest(&fixture).await, stable);
    assert_eq!(physical_digest(&fixture).await, before);
    db.close().await.unwrap();
}

async fn observe(
    db: &Database,
    scope: HubSubmissionScope,
    row: u32,
    account_prior: HubRecordId,
    target_prior: Option<HubRecordId>,
) -> (CountClassificationCommand, i64) {
    let time = 300 + i64::from(row) * 4;
    let crate::HubMutationResult::CorrectionObserved(observation) = Box::pin(
        db.submit_hub_mutation(
            scope,
            operation(220000 + row * 2),
            HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                previous: None,
                facts: CorrectionObservedFacts::CountClassification {
                    explanation_operation: id(801),
                    corrected_packs: WholePacks::from_value(if row % 2 == 1 { 0 } else { 1 })
                        .unwrap(),
                    report: None,
                    basis: SourceProvenance::new(
                        "Independent corrected count classification evidence",
                    )
                    .unwrap(),
                },
            })),
            at(time),
        ),
    )
    .await
    .unwrap() else {
        panic!("ordinary factual observation row{row}")
    };
    (
        CountClassificationCommand {
            observation_operation: observation.original.operation,
            expected_stock_revision: observation.original.stock_revision,
            expected_catalog_revision: observation.original.catalog_revision,
            expected_account_prior: Some(account_prior),
            expected_prior: target_prior,
        },
        time,
    )
}
