use super::*;
mod composition_tests;
mod count_return_tests;
mod fault_tests;
mod lifecycle_tests;
use crate::hub::stock::tests::{Fixture, id, operation, other, owner};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::hub::{
    actuals::{ActualReportCommand, ActualReportSources},
    corrections::{
        AdditionalQuantityCommand, CorrectedLineFacts, CorrectionObservedFacts,
        DepartureQuantityDelta, FoundCorrectionCommand, FoundStockInput, PhysicalDebitKey,
    },
    movement::PhysicalIdentification,
    reference::{HubLabel, SourceProvenance, WholePacks},
    returns::ReturnDepartureClaim,
    stock::Occurrence,
};
fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}

async fn scope(db: &Database) -> HubSubmissionScope {
    let runtime = db.begin_hub_runtime(at(190)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(190),
        )
        .await
        .unwrap();
    HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    }
}

#[tokio::test]
async fn actual_total_four_posts_only_two_preserves_original_and_recovers_after_reopen() {
    let (fixture, db, command) = super::additional_review::tests::actual_world().await;
    let raw = db
        .hub_actual_report_operation(owner(), operation(1800))
        .await
        .unwrap();
    let target = crate::hub::actuals::common::operation(command.facts.target_operation).unwrap();
    let original = db
        .hub_actual_review_operation(owner(), target.clone())
        .await
        .unwrap();
    let scope = scope(&db).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.submit_hub_mutation(
            HubSubmissionScope {
                actor: other(),
                ..scope
            },
            operation(1901),
            HubMutation::AdditionalQuantity(Box::new(command.clone())),
            at(200)
        )
        .await
        .unwrap_err()
        .code(),
        harvestcircle_domain::SafeErrorCode::HubPermissionDenied
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    let crate::HubMutationResult::AdditionalCorrected(receipt) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::AdditionalQuantity(Box::new(command.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("additional native receipt")
    };
    assert_eq!(receipt.original.amendment.before.value(), 2);
    assert_eq!(receipt.original.amendment.after.value(), 4);
    assert_eq!(
        receipt.original.amendment.delta,
        DepartureQuantityDelta::AdditionalDeparture(WholePacks::from_value(2).unwrap())
    );
    assert_eq!(receipt.original.reduction.before.packs.value(), 18);
    assert_eq!(receipt.original.reduction.on_hand_after.value(), 16);
    let group = db
        .hub_group(owner(), command.balancing_group)
        .await
        .unwrap();
    assert_eq!(group.group.packs.value(), 16);
    let reports = db
        .hub_actual_reports(
            owner(),
            crate::HubActualReportQuery {
                expected_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64,
                filter: crate::HubActualReportFilter::All,
            },
        )
        .await
        .unwrap();
    let row = reports
        .rows
        .iter()
        .find(|r| r.report.operation == operation(1800))
        .unwrap();
    assert_eq!(row.review_operation, Some(operation(1900)));
    assert_eq!(
        row.resolution_kind,
        Some(crate::HubActualReportResolutionKind::AdditionalQuantity)
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(1800))
            .await
            .unwrap(),
        raw
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), target.clone())
            .await
            .unwrap(),
        original
    );
    let crate::HubMutationResult::AdditionalCorrected(replayed) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::AdditionalQuantity(Box::new(command)),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("same original")
    };
    assert_eq!(replayed, receipt);
    assert_eq!(
        db.hub_additional_quantity_original(other(), operation(1900))
            .await
            .unwrap_err()
            .code(),
        harvestcircle_domain::SafeErrorCode::HubPermissionDenied
    );
    let after = crate::hub::custody::tests::material(&fixture).await;
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_additional_quantity_original(owner(), operation(1900))
            .await
            .unwrap()
            .unwrap(),
        *receipt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, after);
    db.close().await.unwrap();
}

pub(in crate::hub::corrections) async fn normal_world(
    returned: bool,
    completed: bool,
) -> (
    Fixture,
    Database,
    AdditionalQuantityCommand,
    HubSubmissionScope,
    crate::HubDepartureReceipt,
) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, departure) =
        crate::hub::handling::departure::tests::correction_departure_known_birth(&db).await;
    if returned {
        super::projection_native_tests::return_funding_tests::record_return(
            &db,
            Some(ReturnDepartureClaim {
                departure: departure.record.id,
                group: Some(departure.record.consumed[0].before.id),
            }),
            120,
        )
        .await;
    }
    if completed {
        use harvestcircle_domain::hub::movement::MovementCommand;
        // Real native prerequisite movements complete the original work before
        // the raw report exists. No uncertain planned action is bypassed.
        for (n, time, to) in [(1860, 130, id(7)), (1861, 140, id(6))] {
            let current = db
                .hub_group(owner(), intake.original.position.group)
                .await
                .unwrap();
            db.apply_hub_movement(
                owner(),
                operation(n),
                MovementCommand {
                    expected_stock_revision: current.revision,
                    expected_catalog_revision: current.catalog_revision,
                    group: current.group.id,
                    expected_group_revision: current.group.revision,
                    to,
                    packs: current.group.packs,
                    separated_portion: None,
                    occurred: Occurrence::Known(at(time)),
                    observation: note("Actual completed handling before any corrected-total claim"),
                },
                at(time),
            )
            .await
            .unwrap();
        }
    }
    let runtime = db.begin_hub_runtime(at(170)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(170),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(1800),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(intake.original.position.variant),
                recipient: Some(id(8)),
                packs: Some(WholePacks::from_value(8).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: note("Independently confirmed total eight for original line six"),
            })),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("raw report")
    };
    let current = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let command = AdditionalQuantityCommand {
        facts: CorrectedLineFacts {
            report: report.original.id,
            target_operation: crate::hub::actuals::common::operation_id(&departure.operation)
                .unwrap(),
            target_group: departure.record.consumed[0].before.id,
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            expected_prior: None,
            corrected_line_packs: WholePacks::from_value(8).unwrap(),
            recipient: Some(id(8)),
            occurred: Occurrence::Known(at(100)),
            source_evidence: note(
                "Canonical known-time receipt and exact original physical cohort",
            ),
            quantity_evidence: note("Eight is the corrected total, two is the physical delta"),
            recipient_evidence: note("Immutable original recipient A"),
            timing_evidence: note("Actual departure at the original native observation time"),
        },
        source_operation: crate::hub::actuals::common::operation_id(&intake.operation).unwrap(),
        source_group: intake.original.position.group,
        balancing_group: current.group.id,
        expected_balancing_revision: current.group.revision,
        duplicate_review_basis: note("Complete original event retained; no independent departure"),
    };
    (fixture, db, command, scope, departure)
}

async fn funding(
    db: &Database,
    target: &crate::HubDepartureReceipt,
) -> crate::HubCorrectionFundingEvidence {
    db.hub_correction_target_funding(
        owner(),
        crate::HubCorrectionFundingQuery {
            target_operation: target.operation.clone(),
            original_group: target.record.consumed[0].before.id,
            expected_stock_revision: None,
            expected_catalog_revision: None,
        },
    )
    .await
    .unwrap()
}

async fn found(
    db: &Database,
    scope: HubSubmissionScope,
    target: &DurableRequestId,
    target_group: HubRecordId,
    debit: PhysicalDebitKey,
    number: u32,
    time: i64,
) -> Box<crate::HubFoundCorrectionReceipt> {
    let input = CorrectionObservationCommand {
        previous: None,
        facts: CorrectionObservedFacts::Found {
            target_operation: crate::hub::actuals::common::operation_id(target).unwrap(),
            target_group,
            basis: note("Independent exact physical debit recovered, not a return"),
            allocations: vec![FoundStockInput {
                debit,
                packs: WholePacks::from_value(2).unwrap(),
                location: id(6),
                identification: PhysicalIdentification {
                    label: HubLabel::new(format!("Independent found debit {number}")).unwrap(),
                    observation: note("New restricted physical identity"),
                },
                cohort_evidence: note("Exact original physical debit key independently confirmed"),
            }],
        },
    };
    let crate::HubMutationResult::CorrectionObserved(observed) = db
        .submit_hub_mutation(
            scope,
            operation(number),
            HubMutation::CorrectionObservation(Box::new(input)),
            at(time),
        )
        .await
        .unwrap()
    else {
        panic!("raw found observation")
    };
    let review = db
        .hub_found_correction_review(
            owner(),
            crate::HubFoundCorrectionReviewQuery {
                observation_operation: operation(number),
                proposed_operation: operation(number + 1),
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
            at(time + 1),
        )
        .await
        .unwrap();
    assert!(
        matches!(
            review.outcome,
            crate::HubFoundCorrectionReviewOutcome::Candidate(_)
        ),
        "{:?}",
        review.outcome
    );
    let crate::HubMutationResult::FoundCorrected(result) = db
        .submit_hub_mutation(
            scope,
            operation(number + 1),
            HubMutation::FoundCorrection(Box::new(FoundCorrectionCommand {
                observation_operation: observed.original.operation,
                expected_stock_revision: review.revision,
                expected_catalog_revision: review.catalog_revision,
            })),
            at(time + 1),
        )
        .await
        .unwrap()
    else {
        panic!("found result")
    };
    result
}

#[tokio::test]
async fn normal_additional_delta_and_returns_keep_independent_debit_funding_through_found_and_reopen()
 {
    for returned in [false, true] {
        let (fixture, db, command, scope, departure) = normal_world(returned, false).await;
        let balancing = command.balancing_group;
        let crate::HubMutationResult::AdditionalCorrected(additional) = db
            .submit_hub_mutation(
                scope,
                operation(1900),
                HubMutation::AdditionalQuantity(Box::new(command.clone())),
                at(200),
            )
            .await
            .unwrap()
        else {
            panic!("additional normal")
        };
        assert_eq!(additional.original.reduction.on_hand_after.value(), 12);
        assert_eq!(
            db.hub_departure_operation(owner(), departure.operation.clone())
                .await
                .unwrap(),
            Some(departure.clone())
        );
        let initial = funding(&db, &departure).await;
        assert_eq!(initial.quantity.effective().value(), 8);
        assert_eq!(initial.debits.len(), 2);
        assert_eq!(
            initial.debits[0].available().unwrap().value(),
            if returned { 4 } else { 6 }
        );
        assert_eq!(initial.debits[1].available().unwrap().value(), 2);
        assert_eq!(initial.debits[1].original().key.group, balancing);
        let b = initial.debits[1].original().key;
        let found_b = found(
            &db,
            scope,
            &departure.operation,
            departure.record.consumed[0].before.id,
            b,
            2000,
            210,
        )
        .await;
        let current = funding(&db, &departure).await;
        assert_eq!(current.quantity.effective().value(), 6);
        assert_eq!(
            current.debits[0].available().unwrap().value(),
            if returned { 4 } else { 6 }
        );
        assert_eq!(current.debits[1].available().unwrap().value(), 0);
        let a = current.debits[0].original().key;
        let found_a = found(
            &db,
            scope,
            &departure.operation,
            departure.record.consumed[0].before.id,
            a,
            2010,
            220,
        )
        .await;
        let current = funding(&db, &departure).await;
        assert_eq!(current.quantity.effective().value(), 4);
        assert_eq!(
            current.debits[0].available().unwrap().value(),
            if returned { 2 } else { 4 }
        );
        assert_eq!(current.debits[1].available().unwrap().value(), 0);
        let trace = additional.original.original_line.trace;
        let snapshot = db
            .hub_trace_snapshot(
                owner(),
                crate::HubTraceSnapshotQuery {
                    trace,
                    expected_revision: None,
                    expected_catalog_revision: None,
                },
            )
            .await
            .unwrap();
        let balance = snapshot.balance().unwrap();
        assert_eq!(
            (
                balance.original.value(),
                balance.on_hand.value(),
                balance.departed.value()
            ),
            (20, 16, 4)
        );
        assert_eq!(snapshot.exits[0].packs.value(), 6);
        let history = db
            .hub_trace_history(
                owner(),
                crate::HubTraceHistoryQuery {
                    trace,
                    expected_revision: None,
                    expected_catalog_revision: None,
                    offset: 0,
                    limit: 64,
                },
            )
            .await
            .unwrap();
        let event = history
            .rows
            .iter()
            .find(|e| e.operation == operation(1900))
            .unwrap();
        assert_eq!(event.kind, crate::HubTraceHistoryKind::AdditionalCorrected);
        assert_eq!(event.occurred, Some(command.facts.occurred));
        assert!(history.rows.iter().any(|e| e.operation == operation(1800)));
        db.close().await.unwrap();
        let db = fixture.open().await;
        assert_eq!(
            db.hub_additional_quantity_original(owner(), operation(1900))
                .await
                .unwrap(),
            Some(*additional)
        );
        assert_eq!(
            db.hub_found_correction_original(owner(), found_b.operation.clone())
                .await
                .unwrap(),
            Some(*found_b)
        );
        assert_eq!(
            db.hub_found_correction_original(owner(), found_a.operation.clone())
                .await
                .unwrap(),
            Some(*found_a)
        );
        assert_eq!(funding(&db, &departure).await, current);
        db.close().await.unwrap();
    }
}
