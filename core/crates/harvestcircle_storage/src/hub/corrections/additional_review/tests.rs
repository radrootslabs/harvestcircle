use super::*;
mod duplicate;
use crate::hub::stock::tests::{Fixture, id, operation, other, owner};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        actuals::{ActualReportCommand, ActualReportSources},
        corrections::{CorrectedLineFacts, DepartureQuantityDelta},
        reference::{SourceProvenance, WholePacks},
        stock::Occurrence,
    },
};

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note(s: &str) -> SourceProvenance {
    SourceProvenance::new(s).unwrap()
}

pub(in crate::hub) async fn actual_world() -> (Fixture, Database, AdditionalQuantityCommand) {
    actual_world_configuration(Some(4), false).await
}

async fn actual_world_configuration(
    raw_packs: Option<u32>,
    unknown_hold: bool,
) -> (Fixture, Database, AdditionalQuantityCommand) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, target, _) = if unknown_hold {
        super::super::projection_native_tests::actual_debit_unknown_hold(&db).await
    } else {
        super::super::projection_native_tests::actual_debit(&db).await
    };
    let command = actual_command(&db, before, target, raw_packs).await;
    (fixture, db, command)
}

async fn actual_command(
    db: &Database,
    before: PhysicalStockGroup,
    target: DurableRequestId,
    raw_packs: Option<u32>,
) -> AdditionalQuantityCommand {
    let runtime = db.begin_hub_runtime(at(175)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(175),
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
                variant: Some(before.variant),
                recipient: Some(id(3)),
                packs: raw_packs.map(|v| WholePacks::from_value(v).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: note("Independent corrected whole-line total four; not four more"),
            })),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("preserved corrected total")
    };
    let current = db.hub_group(owner(), before.id).await.unwrap();
    AdditionalQuantityCommand {
        facts: CorrectedLineFacts {
            report: report.original.id,
            target_operation: crate::hub::actuals::common::operation_id(&target).unwrap(),
            target_group: before.id,
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            expected_prior: None,
            corrected_line_packs: WholePacks::from_value(4).unwrap(),
            recipient: Some(id(3)),
            occurred: Occurrence::Known(at(110)),
            source_evidence: note("Exact original receipt and independent physical cohort"),
            quantity_evidence: note("The original two plus two additional whole packs"),
            recipient_evidence: note("Same immutable original recipient"),
            timing_evidence: note("Independently confirmed original departure time"),
        },
        source_operation: crate::hub::actuals::common::operation_id(&operation(1)).unwrap(),
        source_group: before.id,
        balancing_group: before.id,
        expected_balancing_revision: current.group.revision,
        duplicate_review_basis: note("Complete native history checked, exact event amended"),
    }
}

#[tokio::test]
async fn reviewed_actual_total_four_after_two_proposes_only_two_without_mutation_and_reopens() {
    let (fixture, db, command) = actual_world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let review = db
        .hub_additional_quantity_review(owner(), operation(1900), command.clone(), at(200))
        .await
        .unwrap();
    let HubAdditionalQuantityReviewOutcome::Candidate(plan) = &review.outcome else {
        panic!("candidate: {:?}", review.outcome)
    };
    assert_eq!(plan.facts.corrected_line_packs.value(), 4);
    assert_eq!(plan.original_line.packs.value(), 2);
    assert_eq!(plan.amendment.before.value(), 2);
    assert_eq!(plan.amendment.after.value(), 4);
    assert_eq!(
        plan.amendment.delta,
        DepartureQuantityDelta::AdditionalDeparture(WholePacks::from_value(2).unwrap())
    );
    assert_eq!(plan.reduction.before.packs.value(), 18);
    assert_eq!(plan.reduction.actual_departed.value(), 2);
    assert_eq!(plan.report, review.report);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), command, at(200))
            .await
            .unwrap(),
        review
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn current_owner_cuts_original_total_balancing_revision_and_clock_remain_independent() {
    let (fixture, db, command) = actual_world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_additional_quantity_review(other(), operation(1900), command.clone(), at(200))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut stale = command.clone();
    stale.facts.expected_stock_revision = HubRevision::INITIAL;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), stale, at(200))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    let mut total = command.clone();
    total.facts.corrected_line_packs = WholePacks::from_value(2).unwrap();
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), total, at(200))
            .await
            .unwrap()
            .outcome,
        HubAdditionalQuantityReviewOutcome::Blocked(QuantityEffectError::InvalidFacts)
    );
    let mut balancing = command.clone();
    balancing.expected_balancing_revision = HubRevision::INITIAL;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), balancing, at(200))
            .await
            .unwrap()
            .outcome,
        HubAdditionalQuantityReviewOutcome::Blocked(QuantityEffectError::StaleRevision)
    );
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(81), command.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        HubAdditionalQuantityReviewOutcome::OperationAlreadyUsed
    );
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), command, at(170))
            .await
            .unwrap()
            .outcome,
        HubAdditionalQuantityReviewOutcome::Blocked(QuantityEffectError::InvalidHistory)
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn normal_total_eight_after_six_uses_explicit_current_fourteen_group_and_original_paper() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (intake, departure) =
        crate::hub::handling::departure::tests::correction_departure_known_birth(&db).await;
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
    let before = crate::hub::custody::tests::material(&fixture).await;
    let review = db
        .hub_additional_quantity_review(owner(), operation(1900), command.clone(), at(200))
        .await
        .unwrap();
    let HubAdditionalQuantityReviewOutcome::Candidate(plan) = &review.outcome else {
        panic!("normal candidate: {:?}", review.outcome)
    };
    assert_eq!(
        (
            plan.amendment.before.value(),
            plan.amendment.after.value(),
            plan.reduction.actual_departed.value()
        ),
        (6, 8, 2)
    );
    assert_eq!(plan.reduction.before.packs.value(), 14);
    assert_eq!(plan.reduction.before.id, intake.original.position.group);
    assert_ne!(
        plan.reduction.before.id,
        departure.record.consumed[0].before.id
    );
    assert_eq!(plan.report, report.original);
    let mut unknown_time = command.clone();
    unknown_time.facts.occurred = Occurrence::Unknown;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), unknown_time, at(200))
            .await
            .unwrap()
            .outcome,
        HubAdditionalQuantityReviewOutcome::Blocked(QuantityEffectError::Actual(
            harvestcircle_domain::hub::actuals::ActualReportError::HistoricalSourceUnresolved
        ))
    );
    let mut wrong_source = command.clone();
    wrong_source.source_operation = id(99);
    assert!(
        db.hub_additional_quantity_review(owner(), operation(1900), wrong_source, at(200))
            .await
            .is_err()
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), command, at(200))
            .await
            .unwrap(),
        review
    );
    assert_eq!(
        db.hub_departure_operation(owner(), departure.operation.clone())
            .await
            .unwrap(),
        Some(departure)
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn already_reviewed_raw_report_cannot_be_reused_for_a_quantity_delta() {
    use harvestcircle_domain::hub::actuals::ActualDuplicateLink;
    let (fixture, db, mut command) = actual_world_configuration(None, false).await;
    let group = db
        .hub_group(owner(), command.balancing_group)
        .await
        .unwrap();
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
    db.submit_hub_mutation(
        HubSubmissionScope {
            runtime,
            generation,
            actor: owner(),
        },
        operation(1850),
        HubMutation::ActualLink(Box::new(ActualDuplicateLink {
            report: command.facts.report,
            known_operation: command.facts.target_operation,
            known_trace: group.group.trace,
            known_variant: group.group.variant,
            verification: note(
                "Independent review linked the unknown raw total to the original event",
            ),
        })),
        at(190),
    )
    .await
    .unwrap();
    let current = db
        .hub_group(owner(), command.balancing_group)
        .await
        .unwrap();
    command.facts.expected_stock_revision = current.revision;
    command.facts.expected_catalog_revision = current.catalog_revision;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), command.clone(), at(200))
            .await
            .unwrap()
            .outcome,
        HubAdditionalQuantityReviewOutcome::Blocked(QuantityEffectError::AlreadyReviewed)
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_additional_quantity_review(owner(), operation(1900), command, at(200))
            .await
            .unwrap()
            .outcome,
        HubAdditionalQuantityReviewOutcome::Blocked(QuantityEffectError::AlreadyReviewed)
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_verified_scope_preserves_unknown_hold_timing_without_claiming_clearance() {
    use harvestcircle_domain::hub::actuals::{HistoricalHoldApplicability, HistoricalHoldTiming};
    let (fixture, db, command) = actual_world_configuration(Some(4), true).await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let review = db
        .hub_additional_quantity_review(owner(), operation(1900), command, at(200))
        .await
        .unwrap();
    let HubAdditionalQuantityReviewOutcome::Candidate(plan) = review.outcome else {
        panic!("held actual candidate")
    };
    assert_eq!(plan.potential_historical_holds.len(), 1);
    assert_eq!(
        plan.potential_historical_holds[0].conditional_timing,
        HistoricalHoldTiming::TimingUnknown
    );
    assert_eq!(
        plan.potential_historical_holds[0].applicability,
        HistoricalHoldApplicability::HistoricalScopeUnverified
    );
    assert_eq!(review.historical_holds.len(), 1);
    let HistoricalHoldScopeAssessment::Verified(scope) = &review.historical_holds[0] else {
        panic!("canonical original cohort")
    };
    assert_eq!(scope.timing(), HistoricalHoldTiming::TimingUnknown);
    assert!(!scope.births().is_empty());
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn same_revision_siblings_keep_exact_selected_debit_and_narrow_hold_scope() {
    use harvestcircle_domain::hub::{
        actuals::{HistoricalHoldScopeUncertainty, HistoricalHoldTiming},
        movement::{HandlingCommand, PhysicalIdentification},
        portions::SplitCommand,
        reference::HubLabel,
        treatment::{TreatmentAction, TreatmentCommand, TreatmentSelection},
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before_group, target, _) = super::super::projection_native_tests::actual_debit(&db).await;
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
    let mut children = Vec::new();
    for (index, time) in [(0, 170), (1, 171)] {
        let current = db.hub_group(owner(), before_group.id).await.unwrap();
        let crate::HubMutationResult::Handling(receipt) = db
            .submit_hub_mutation(
                scope,
                operation(1860 + index),
                HubMutation::Handling(Box::new(HandlingCommand::Split(SplitCommand {
                    expected_stock_revision: current.revision,
                    expected_catalog_revision: current.catalog_revision,
                    group: before_group.id,
                    expected_group_revision: current.group.revision,
                    packs: WholePacks::from_value(2).unwrap(),
                    identification: PhysicalIdentification {
                        label: HubLabel::new(format!("Independently identified sibling {index}"))
                            .unwrap(),
                        observation: note("Actual separate original cohort branch"),
                    },
                    occurred: Occurrence::Known(at(110)),
                    observation: note("Independently observed physical separation"),
                }))),
                at(time),
            )
            .await
            .unwrap()
        else {
            panic!("native split")
        };
        let crate::HubHandlingResult::Separated(result) = receipt.result else {
            panic!("separated")
        };
        children.push(result.portion_group);
    }
    let held = db.hub_group(owner(), children[0]).await.unwrap();
    db.submit_hub_mutation(
        scope,
        operation(1862),
        HubMutation::Handling(Box::new(HandlingCommand::Treatment(TreatmentCommand {
            expected_stock_revision: held.revision,
            expected_catalog_revision: held.catalog_revision,
            action: TreatmentAction::PlaceHold {
                selection: TreatmentSelection::new(vec![children[0]]).unwrap(),
            },
            occurred: Occurrence::Unknown,
            observation: note("Only the first identified sibling is held; timing unknown"),
        }))),
        at(172),
    )
    .await
    .unwrap();
    let mut first = actual_command(&db, before_group, target, Some(4)).await;
    let b = db.hub_group(owner(), children[0]).await.unwrap();
    let c = db.hub_group(owner(), children[1]).await.unwrap();
    assert_eq!(b.group.revision, c.group.revision);
    first.balancing_group = b.group.id;
    first.expected_balancing_revision = b.group.revision;
    let mut second = first.clone();
    second.balancing_group = c.group.id;
    let hub = db.load_hub().await.unwrap().unwrap().id();
    let own = crate::hub::actuals::common::operation_id(&operation(1900)).unwrap();
    assert_ne!(
        first.fingerprint(hub, owner(), own),
        second.fingerprint(hub, owner(), own)
    );
    let material = crate::hub::custody::tests::material(&fixture).await;
    let reviewed_b = db
        .hub_additional_quantity_review(owner(), operation(1900), first.clone(), at(200))
        .await
        .unwrap();
    let reviewed_c = db
        .hub_additional_quantity_review(owner(), operation(1900), second.clone(), at(200))
        .await
        .unwrap();
    let HubAdditionalQuantityReviewOutcome::Candidate(plan_b) = &reviewed_b.outcome else {
        panic!("B candidate: {:?}", reviewed_b.outcome)
    };
    let HubAdditionalQuantityReviewOutcome::Candidate(plan_c) = &reviewed_c.outcome else {
        panic!("C candidate: {:?}", reviewed_c.outcome)
    };
    assert_eq!(plan_b.reduction.before.id, b.group.id);
    assert_eq!(plan_c.reduction.before.id, c.group.id);
    assert_eq!(reviewed_b.historical_holds.len(), 1);
    assert_eq!(reviewed_c.historical_holds.len(), 1);
    assert!(
        matches!(&reviewed_b.historical_holds[0], HistoricalHoldScopeAssessment::Verified(scope) if scope.timing()==HistoricalHoldTiming::TimingUnknown)
    );
    assert!(matches!(
        &reviewed_c.historical_holds[0],
        HistoricalHoldScopeAssessment::Unresolved {
            reason: HistoricalHoldScopeUncertainty::ScopeNotEstablished,
            conditional_timing: HistoricalHoldTiming::TimingUnknown,
            ..
        }
    ));
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let context = Box::pin(load_on(tx, hub, &second)).await?;
                assert_eq!(
                    first
                        .plan(&context.domain(), own, owner(), at(200))
                        .unwrap_err(),
                    QuantityEffectError::InvalidScope
                );
                Ok::<(), SafeError>(())
            })
        })
        .await
        .unwrap();
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
    db.close().await.unwrap();
}
