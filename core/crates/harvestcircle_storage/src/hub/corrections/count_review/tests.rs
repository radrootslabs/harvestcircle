use super::*;
use crate::hub::stock::tests::{Fixture, command, id, operation, other, owner, setup};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        actuals::{ActualReportCommand, ActualReportSources, ReportedActualSource},
        counts::{CountObserveCommand, CountPostCommand},
        movement::{HandlingCommand, PhysicalIdentification},
        portions::SplitCommand,
        reference::{HubLabel, SourceProvenance, WholePacks},
        stock::Occurrence,
    },
};

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Independent fictional evidence for counted missing stock").unwrap()
}
fn packs(n: u32) -> WholePacks {
    WholePacks::from_value(n).unwrap()
}

pub(in crate::hub) async fn world() -> (Fixture, Database, HubCountExplanationQuery) {
    world_with_later_split(false).await
}

async fn world_with_later_split(split: bool) -> (Fixture, Database, HubCountExplanationQuery) {
    world_with_history(split, false).await
}

async fn world_with_history(
    split: bool,
    sibling_count: bool,
) -> (Fixture, Database, HubCountExplanationQuery) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let runtime = db.begin_hub_runtime(at(100)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(100),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let mut input = command(&catalog);
    input.occurred = Occurrence::Known(at(40));
    let crate::HubMutationResult::Received(intake) = db
        .submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(input)),
            at(100),
        )
        .await
        .unwrap()
    else {
        panic!("native receipt")
    };
    let group = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let crate::HubMutationResult::CountObserved(observed) = db
        .submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::CountObservation(Box::new(CountObserveCommand {
                expected_stock_revision: group.revision,
                expected_catalog_revision: group.catalog_revision,
                group: group.group.id,
                expected_group_revision: group.group.revision,
                counted_packs: packs(18),
                cutoff: Occurrence::Known(at(110)),
                observation: note(),
            })),
            at(120),
        )
        .await
        .unwrap()
    else {
        panic!("native count observation")
    };
    let reviewed = db
        .hub_count_review(owner(), observed.original.id)
        .await
        .unwrap();
    let crate::HubMutationResult::CountPosted(posted) = db
        .submit_hub_mutation(
            scope,
            operation(3),
            HubMutation::CountPosting(Box::new(CountPostCommand {
                expected_stock_revision: reviewed.stock_revision,
                expected_catalog_revision: reviewed.catalog_revision,
                count: observed.original.id,
                reviewed_activity: true,
                surplus_identification: None,
                reason: note(),
            })),
            at(130),
        )
        .await
        .unwrap()
    else {
        panic!("native count posting")
    };
    if split {
        let group = db
            .hub_group(owner(), intake.original.position.group)
            .await
            .unwrap();
        let crate::HubMutationResult::Handling(separated) = db
            .submit_hub_mutation(
                scope,
                operation(6),
                HubMutation::Handling(Box::new(HandlingCommand::Split(SplitCommand {
                    expected_stock_revision: group.revision,
                    expected_catalog_revision: group.catalog_revision,
                    group: group.group.id,
                    expected_group_revision: group.group.revision,
                    packs: packs(2),
                    identification: PhysicalIdentification {
                        label: HubLabel::new("Later independently marked two crates").unwrap(),
                        observation: note(),
                    },
                    occurred: Occurrence::Known(at(131)),
                    observation: note(),
                }))),
                at(132),
            )
            .await
            .unwrap()
        else {
            panic!("native split")
        };
        if sibling_count {
            let crate::HubHandlingResult::Separated(separated) = separated.result else {
                panic!("split result")
            };
            post_count(&db, scope, separated.portion_group, 1, 10, 133).await;
            let state = db
                .hub_group(owner(), intake.original.position.group)
                .await
                .unwrap();
            let mut other = command(&catalog);
            other.expected_stock_revision = state.revision;
            db.submit_hub_mutation(
                scope,
                operation(9),
                HubMutation::Receive(Box::new(other)),
                at(137),
            )
            .await
            .unwrap();
        }
    }
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(4),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(id(2)),
                recipient: Some(id(3)),
                packs: Some(packs(1)),
                sources: ActualReportSources::EstablishedClaim(ReportedActualSource {
                    trace: intake.original.trace.id,
                    group: Some(group.group.id),
                    location: Some(id(6)),
                }),
                occurred: Occurrence::Known(at(70)),
                observation: note(),
            })),
            at(140),
        )
        .await
        .unwrap()
    else {
        panic!("native raw report")
    };
    let current = db.hub_group(owner(), group.group.id).await.unwrap();
    let query = HubCountExplanationQuery {
        facts: CountExplanationFacts {
            report: report.original.id,
            count_reconciliation: posted.original.id,
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            expected_prior: None,
            packs: packs(1),
            recipient: id(3),
            occurred: Occurrence::Known(at(70)),
            source_evidence: note(),
            quantity_evidence: note(),
            recipient_evidence: note(),
            timing_evidence: note(),
        },
        source_operation: crate::hub::actuals::common::operation_id(&intake.operation).unwrap(),
        source_group: group.group.id,
    };
    (fixture, db, query)
}

async fn post_count(
    db: &Database,
    scope: HubSubmissionScope,
    group: HubRecordId,
    counted: u32,
    identity: u32,
    time: i64,
) {
    let group = db.hub_group(owner(), group).await.unwrap();
    let crate::HubMutationResult::CountObserved(observed) = db
        .submit_hub_mutation(
            scope,
            operation(identity),
            HubMutation::CountObservation(Box::new(CountObserveCommand {
                expected_stock_revision: group.revision,
                expected_catalog_revision: group.catalog_revision,
                group: group.group.id,
                expected_group_revision: group.group.revision,
                counted_packs: packs(counted),
                cutoff: Occurrence::Known(at(time)),
                observation: note(),
            })),
            at(time),
        )
        .await
        .unwrap()
    else {
        panic!("sibling count")
    };
    let reviewed = db
        .hub_count_review(owner(), observed.original.id)
        .await
        .unwrap();
    db.submit_hub_mutation(
        scope,
        operation(identity + 1),
        HubMutation::CountPosting(Box::new(CountPostCommand {
            expected_stock_revision: reviewed.stock_revision,
            expected_catalog_revision: reviewed.catalog_revision,
            count: observed.original.id,
            reviewed_activity: true,
            surplus_identification: None,
            reason: note(),
        })),
        at(time + 1),
    )
    .await
    .unwrap();
}

async fn graph_at(
    db: &Database,
    trace: HubRecordId,
    cut: HubRevision,
) -> Result<harvestcircle_domain::hub::portions::PhysicalLineage, SafeError> {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                Box::pin(handling::lineage::historical_quantities_on(
                    tx, hub, trace, cut,
                ))
                .await
            })
        })
        .await
        .map_err(map_transaction_error)
}

#[tokio::test]
async fn count_cut_reconstruction_detects_hidden_found_birth_without_poisoning_the_earlier_cut() {
    let (fixture, db, found) =
        super::super::projection_native_tests::found_review_tests::posting_tests::posted_fixture()
            .await;
    let trace = found.original.after_lineage.trace();
    let cut = found.original.after_lineage.revision();
    let earlier = HubRevision::from_value(cut.value() - 1).unwrap();
    let earlier_graph = graph_at(&db, trace, earlier).await.unwrap();
    assert_eq!(
        graph_at(&db, trace, cut).await.unwrap(),
        found.original.after_lineage
    );
    crate::hub::custody::tests::corrupt(&fixture, "hub_group_identity_immutable",
        "UPDATE hub_stock_groups SET trace_id=(SELECT trace_id FROM hub_receipts WHERE origin_kind='return' ORDER BY trace_id LIMIT 1) WHERE group_id IN (SELECT child_group_id FROM hub_correction_found_allocations)").await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        graph_at(&db, trace, cut).await.unwrap_err().code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(graph_at(&db, trace, earlier).await.unwrap(), earlier_graph);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn count_comparison_cannot_relabel_an_overlapping_known_actual_departure_as_count_loss() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (before, _, _) = super::super::projection_native_tests::actual_debit(&db).await;
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
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    post_count(&db, scope, before.id, 16, 600, 200).await;
    let count = db
        .hub_count_post_operation(owner(), operation(601))
        .await
        .unwrap()
        .unwrap();
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(602),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(before.variant),
                recipient: Some(id(3)),
                packs: Some(packs(1)),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Known(at(110)),
                observation: note(),
            })),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("raw count explanation report")
    };
    let current = db.hub_group(owner(), before.id).await.unwrap();
    let mut query = HubCountExplanationQuery {
        facts: CountExplanationFacts {
            report: report.original.id,
            count_reconciliation: count.original.id,
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            expected_prior: None,
            packs: packs(1),
            recipient: id(3),
            occurred: Occurrence::Known(at(110)),
            source_evidence: note(),
            quantity_evidence: note(),
            recipient_evidence: note(),
            timing_evidence: note(),
        },
        source_operation: crate::hub::actuals::common::operation_id(&operation(1)).unwrap(),
        source_group: before.id,
    };
    let material = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(603), query.clone(), at(220))
            .await
            .unwrap()
            .outcome,
        HubCountExplanationReviewOutcome::Blocked(QuantityEffectError::Actual(
            harvestcircle_domain::hub::actuals::ActualReportError::DuplicateReviewRequired
        ))
    );
    query.facts.report = db
        .hub_actual_report_operation(owner(), operation(80))
        .await
        .unwrap()
        .unwrap()
        .original
        .id;
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(603), query, at(220))
            .await
            .unwrap()
            .outcome,
        HubCountExplanationReviewOutcome::Blocked(QuantityEffectError::AlreadyReviewed)
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        material
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn count_comparison_cannot_explain_three_packs_from_a_native_two_pack_shortage() {
    let (fixture, db, mut query) = world().await;
    let runtime = db.begin_hub_runtime(at(145)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(145),
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
            operation(7),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(id(2)),
                recipient: Some(id(3)),
                packs: Some(packs(3)),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Known(at(70)),
                observation: note(),
            })),
            at(145),
        )
        .await
        .unwrap()
    else {
        panic!("independent original three-pack report")
    };
    let current = db.hub_group(owner(), query.source_group).await.unwrap();
    query.facts.report = report.original.id;
    query.facts.packs = packs(3);
    query.facts.expected_stock_revision = current.revision;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(5), query.clone(), at(150))
            .await
            .unwrap()
            .outcome,
        HubCountExplanationReviewOutcome::Blocked(QuantityEffectError::Accounting(
            harvestcircle_domain::hub::corrections::CorrectionError::InvalidQuantity
        ))
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(7))
            .await
            .unwrap(),
        Some(*report)
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(5), query, at(150))
            .await
            .unwrap()
            .outcome,
        HubCountExplanationReviewOutcome::Blocked(QuantityEffectError::Accounting(
            harvestcircle_domain::hub::corrections::CorrectionError::InvalidQuantity
        ))
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn count_comparison_and_cut_reconstruction_reject_hidden_sibling_shortage_and_birth_roots() {
    for (trigger, statement) in [
        (
            "hub_stock_events_no_update",
            "UPDATE hub_stock_events SET trace_id=(SELECT trace_id FROM hub_receipts WHERE operation_id='01900000-0000-7000-8000-000000000009') WHERE kind='count_post' AND group_id IN (SELECT group_id FROM hub_group_identification WHERE parent_group_id IS NOT NULL)",
        ),
        (
            "hub_group_identity_immutable",
            "UPDATE hub_stock_groups SET trace_id=(SELECT trace_id FROM hub_receipts WHERE operation_id='01900000-0000-7000-8000-000000000009') WHERE group_id IN (SELECT group_id FROM hub_group_identification WHERE parent_group_id IS NOT NULL)",
        ),
    ] {
        let (fixture, db, query) = world_with_history(true, true).await;
        assert!(matches!(
            db.hub_count_explanation_review(owner(), operation(5), query.clone(), at(150))
                .await
                .unwrap()
                .outcome,
            HubCountExplanationReviewOutcome::Candidate(_)
        ));
        let group = db.hub_group(owner(), query.source_group).await.unwrap();
        let trace = group.group.trace;
        let cut = group.revision;
        crate::hub::custody::tests::corrupt(&fixture, trigger, statement).await;
        let before = crate::hub::custody::tests::material(&fixture).await;
        let error = db
            .host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, owner()).await?.id();
                    Box::pin(handling::lineage::historical_quantities_on(
                        tx, hub, trace, cut,
                    ))
                    .await
                })
            })
            .await
            .unwrap_err();
        assert_eq!(
            map_transaction_error(error).code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            db.hub_count_explanation_review(owner(), operation(5), query, at(150))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn count_comparison_fails_closed_on_damaged_original_count_or_birth_without_writes() {
    for (trigger, sql) in [
        (
            "hub_count_reconciliation_no_update",
            "UPDATE hub_count_reconciliations SET result_hash=zeroblob(32)",
        ),
        (
            "hub_receipts_no_update",
            "UPDATE hub_receipts SET original_packs=21",
        ),
    ] {
        let (fixture, db, query) = world().await;
        crate::hub::custody::tests::corrupt(&fixture, trigger, sql).await;
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.hub_count_explanation_review(owner(), operation(5), query, at(150))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn count_comparison_detects_balanced_sibling_position_transfer_without_repairing_history() {
    use sqlx::Connection;
    let (fixture, db, query) = world_with_later_split(true).await;
    let mut raw = fixture.raw().await;
    // Fresh disposable fixture only. Sum remains eighteen: a population sum
    // cannot detect this fabricated transfer between independently marked stock.
    assert_eq!(
        sqlx::query("UPDATE hub_stock_positions SET packs=CASE WHEN group_id=? THEN 15 ELSE 3 END")
            .bind(query.source_group.as_bytes().as_slice())
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        2
    );
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(5), query, at(150))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_shortage_comparison_keeps_original_twenty_and_eighteen_without_second_effect_after_restart()
 {
    let (fixture, db, query) = world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let reviewed = db
        .hub_count_explanation_review(owner(), operation(5), query.clone(), at(150))
        .await
        .unwrap();
    let HubCountExplanationReviewOutcome::Candidate(plan) = &reviewed.outcome else {
        panic!("{:?}", reviewed.outcome)
    };
    assert_eq!(plan.count.before.packs, packs(20));
    assert!(
        matches!(plan.count.effect, CountEffect::Shortage { packs: value, after_packs, .. } if value==packs(2) && after_packs==packs(18))
    );
    assert_eq!(plan.explanation.packs, packs(1));
    assert_eq!(plan.unexplained_after, packs(1));
    assert_eq!(plan.known_event.lines[0].packs, packs(1));
    assert_eq!(plan.historical_source.historical().group.packs, packs(20));
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(5), query, at(150))
            .await
            .unwrap(),
        reviewed
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn native_comparison_rejects_foreign_stale_missing_source_unknown_time_excess_and_used_operation_without_writes()
 {
    let (fixture, db, query) = world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_count_explanation_review(other(), operation(5), query.clone(), at(150))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubPermissionDenied
    );
    let mut stale = query.clone();
    stale.facts.expected_stock_revision = HubRevision::INITIAL;
    assert!(
        db.hub_count_explanation_review(owner(), operation(5), stale, at(150))
            .await
            .is_err()
    );
    let mut missing = query.clone();
    missing.source_operation = id(99);
    assert!(
        db.hub_count_explanation_review(owner(), operation(5), missing, at(150))
            .await
            .is_err()
    );
    let mut unknown = query.clone();
    unknown.facts.occurred = Occurrence::Unknown;
    assert!(matches!(
        db.hub_count_explanation_review(owner(), operation(5), unknown, at(150))
            .await
            .unwrap()
            .outcome,
        HubCountExplanationReviewOutcome::Blocked(_)
    ));
    let mut excess = query.clone();
    excess.facts.packs = packs(3);
    assert!(matches!(
        db.hub_count_explanation_review(owner(), operation(5), excess, at(150))
            .await
            .unwrap()
            .outcome,
        HubCountExplanationReviewOutcome::Blocked(_)
    ));
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(4), query, at(150))
            .await
            .unwrap()
            .outcome,
        HubCountExplanationReviewOutcome::OperationAlreadyUsed
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn later_native_split_does_not_change_pre_count_topology_quantities_or_explanation() {
    let (fixture, db, query) = world_with_later_split(true).await;
    let group = db.hub_group(owner(), query.source_group).await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let reviewed = db
        .hub_count_explanation_review(owner(), operation(5), query.clone(), at(170))
        .await
        .unwrap();
    let HubCountExplanationReviewOutcome::Candidate(after_plan) = &reviewed.outcome else {
        panic!("candidate: {:?}", reviewed.outcome)
    };
    assert_eq!(
        after_plan.historical_source.historical().group.packs,
        packs(20)
    );
    assert_eq!(after_plan.count.before.packs, packs(20));
    assert_eq!(after_plan.unexplained_after, packs(1));
    assert_eq!(group.group.packs, packs(16));
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(5), query, at(170))
            .await
            .unwrap(),
        reviewed
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
