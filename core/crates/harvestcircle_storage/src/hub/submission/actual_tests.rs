//! Actual ingress pending evidence survives dropped grants and real session loss.
use super::*;
use crate::hub::stock::tests::{Fixture, command, id, operation, other, owner, setup};
use harvestcircle_application::SessionGeneration;
use harvestcircle_domain::{
    SafeErrorCode, UnixTimestamp,
    hub::{
        actuals::{ActualReportCommand, ActualReportSources},
        movement::{HandlingCommand, MovementCommand},
        reference::SourceProvenance,
        stock::Occurrence,
    },
};

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn report() -> ActualReportCommand {
    ActualReportCommand {
        variant: None,
        recipient: None,
        packs: None,
        sources: ActualReportSources::Unknown,
        occurred: Occurrence::Known(at(160)),
        observation: SourceProvenance::new(
            "Already-taken raw claim; recording clock conflict must remain visible",
        )
        .unwrap(),
    }
}
async fn active_scope(db: &Database) -> HubSubmissionScope {
    let runtime = db.begin_hub_runtime(at(140)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(140),
        )
        .await
        .unwrap();
    HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    }
}
async fn planned(db: &Database, group: HubRecordId) -> HubMutation {
    let current = db.hub_group(owner(), group).await.unwrap();
    HubMutation::Handling(Box::new(HandlingCommand::Move(MovementCommand {
        expected_stock_revision: current.revision,
        expected_catalog_revision: current.catalog_revision,
        group,
        expected_group_revision: current.group.revision,
        to: id(7),
        packs: current.group.packs,
        separated_portion: None,
        occurred: Occurrence::Known(at(180)),
        observation: SourceProvenance::new("A later planned physical action").unwrap(),
    })))
}

#[tokio::test]
async fn forced_rollback_after_complete_actual_writer_keeps_every_original_and_pending_claim() {
    use harvestcircle_domain::hub::actuals::{ActualNewDepartureCommand, ActualReviewFacts};
    use harvestcircle_domain::hub::reference::WholePacks;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), at(100))
        .await
        .unwrap();
    let scope = active_scope(&db).await;
    let mut input = report();
    input.occurred = Occurrence::Unknown;
    input.packs = Some(WholePacks::from_value(2).unwrap());
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(input)),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("report")
    };
    let current = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let evidence = || SourceProvenance::new("Independent native factual verification").unwrap();
    let command = ActualNewDepartureCommand {
        facts: ActualReviewFacts {
            report: report.original.id,
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            group: current.group.id,
            expected_group_revision: current.group.revision,
            packs: WholePacks::from_value(2).unwrap(),
            recipient: id(3),
            occurred: Occurrence::Known(at(110)),
            source_evidence: evidence(),
            quantity_evidence: evidence(),
            recipient_evidence: evidence(),
            timing_evidence: evidence(),
        },
        source_operation: HubRecordId::from_hex(&operation(1).as_str().replace('-', "")).unwrap(),
        source_group: current.group.id,
        duplicate_review_basis: evidence(),
    };
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(81),
                    HubMutation::ActualReview(Box::new(command)),
                    at(160),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("fresh review admission")
    };
    let before = crate::hub::custody::tests::material(&fixture).await;
    let result = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let HubMutation::ActualReview(command) = grant.mutation else {
                    panic!("typed review")
                };
                let receipt = crate::hub::actuals::write::departure_on(
                    tx,
                    grant.scope.actor,
                    grant.operation,
                    *command,
                    at(170),
                )
                .await?;
                assert!(matches!(
                    receipt.original,
                    crate::HubActualReviewOriginal::Departure(_)
                ));
                // Test-only transaction callback fails after every real writer/readback.
                // No schema fault or product injection path is introduced.
                Err::<(), SafeError>(storage_unavailable())
            })
        })
        .await;
    assert!(result.is_err());
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    assert_eq!(
        db.hub_group(owner(), current.group.id).await.unwrap(),
        current
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(80))
            .await
            .unwrap(),
        Some(*report)
    );
    assert!(
        db.hub_actual_review_operation(owner(), operation(81))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.hub_submission(owner(), operation(81))
            .await
            .unwrap()
            .unwrap()
            .status,
        HubSubmissionStatus::Pending
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn dropped_actual_grant_keeps_exact_raw_claim_and_blocks_planned_work_after_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), at(100))
        .await
        .unwrap();
    let scope = active_scope(&db).await;
    let input = report();
    let expected = serde_json::to_value(&input).unwrap();
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(80),
                    HubMutation::ActualReport(Box::new(input)),
                    at(150),
                )
                .await
            })
        })
        .await
        .unwrap();
    assert!(matches!(decision, admission::Decision::Fresh(_)));
    drop(decision);
    let pending = db
        .hub_submission(owner(), operation(80))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, HubSubmissionStatus::Pending);
    let bytes = pending.captured_semantics().to_vec();
    let captured: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(captured["input"], expected);
    assert!(
        db.hub_actual_report_operation(owner(), operation(80))
            .await
            .unwrap()
            .is_none()
    );
    let blocked = db
        .submit_hub_mutation(
            scope,
            operation(81),
            planned(&db, intake.original.position.group).await,
            at(180),
        )
        .await
        .unwrap_err();
    assert_eq!(blocked.code(), SafeErrorCode::HubConflict);
    assert!(
        db.hub_submission(owner(), operation(81))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(report())),
            at(180)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    let fresh = active_scope(&db).await;
    let restored = db
        .hub_submission(owner(), operation(80))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.status, HubSubmissionStatus::Pending);
    assert_eq!(restored.captured_semantics(), bytes);
    assert_eq!(
        db.submit_hub_mutation(
            fresh,
            operation(82),
            planned(&db, intake.original.position.group).await,
            at(190)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap()
            .group
            .packs,
        intake.original.position.packs
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn actual_execution_session_loss_never_turns_committed_raw_fact_into_known_rejection() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), at(100))
        .await
        .unwrap();
    let scope = active_scope(&db).await;
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(80),
                    HubMutation::ActualReport(Box::new(report())),
                    at(150),
                )
                .await
            })
        })
        .await
        .unwrap();
    let admission::Decision::Fresh(grant) = decision else {
        panic!("fresh native admission")
    };
    let original = db
        .hub_submission(owner(), operation(80))
        .await
        .unwrap()
        .unwrap();
    let generation = db
        .advance_hub_session(scope.runtime, scope.generation, Some(other()), at(155))
        .await
        .unwrap();
    assert_eq!(
        execute::execute_fresh(&db, grant, at(160))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    let pending = db
        .hub_submission(owner(), operation(80))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, HubSubmissionStatus::Pending);
    assert_eq!(pending.captured_semantics(), original.captured_semantics());
    assert!(
        db.hub_actual_report_operation(owner(), operation(80))
            .await
            .unwrap()
            .is_none()
    );
    let generation = db
        .advance_hub_session(scope.runtime, generation, Some(owner()), at(170))
        .await
        .unwrap();
    let restored = HubSubmissionScope {
        generation,
        ..scope
    };
    assert_eq!(
        db.submit_hub_mutation(
            restored,
            operation(81),
            planned(&db, intake.original.position.group).await,
            at(180)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap()
            .group
            .packs,
        intake.original.position.packs
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn actual_preview_rejects_pending_candidate_and_never_excludes_another_uncertain_report() {
    use harvestcircle_domain::hub::actuals::{ActualNewDepartureCommand, ActualReviewFacts};
    use harvestcircle_domain::hub::reference::WholePacks;
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let intake = db
        .apply_hub_receipt(owner(), operation(1), command(&catalog), at(100))
        .await
        .unwrap();
    let scope = active_scope(&db).await;
    let crate::HubMutationResult::ActualReported(saved) = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(report())),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("report");
    };
    let group = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let candidate = ActualNewDepartureCommand {
        facts: ActualReviewFacts {
            report: saved.original.id,
            expected_stock_revision: group.revision,
            expected_catalog_revision: group.catalog_revision,
            group: group.group.id,
            expected_group_revision: group.group.revision,
            packs: WholePacks::from_value(2).unwrap(),
            recipient: id(3),
            occurred: Occurrence::Known(at(110)),
            source_evidence: SourceProvenance::new("Original cohort verified").unwrap(),
            quantity_evidence: SourceProvenance::new("Two whole packs verified").unwrap(),
            recipient_evidence: SourceProvenance::new("Recipient independently verified").unwrap(),
            timing_evidence: SourceProvenance::new("Time independently verified").unwrap(),
        },
        source_operation: HubRecordId::from_hex(&operation(1).as_str().replace('-', "")).unwrap(),
        source_group: group.group.id,
        duplicate_review_basis: SourceProvenance::new("Complete native original history checked")
            .unwrap(),
    };
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(82),
                    HubMutation::ActualReport(Box::new(report())),
                    at(151),
                )
                .await
            })
        })
        .await
        .unwrap();
    assert!(matches!(decision, admission::Decision::Fresh(_)));
    drop(decision);
    let pending = db
        .hub_submission(owner(), operation(82))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, HubSubmissionStatus::Pending);
    assert_eq!(
        db.hub_actual_departure_preview(owner(), operation(82), candidate.clone(), at(160))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_actual_departure_preview(owner(), operation(81), candidate, at(160))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_submission(owner(), operation(82)).await.unwrap(),
        Some(pending)
    );
    assert!(
        db.hub_submission(owner(), operation(81))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(db.hub_group(owner(), group.group.id).await.unwrap(), group);
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(80))
            .await
            .unwrap(),
        Some(*saved)
    );
    db.close().await.unwrap();
}
