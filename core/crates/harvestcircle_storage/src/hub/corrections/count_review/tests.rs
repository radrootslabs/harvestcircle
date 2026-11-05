use super::*;

mod capture;
use crate::hub::stock::tests::{Fixture, command, id, operation, other, owner, setup};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::hub::corrections::{CountExplanationFacts, CountLossQuantityAccount};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        actuals::{ActualReportCommand, ActualReportSources, ReportedActualSource},
        counts::{CountEffect, CountObserveCommand, CountPostCommand},
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

pub(in crate::hub) async fn world_with_later_split(
    split: bool,
) -> (Fixture, Database, HubCountExplanationQuery) {
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

#[tokio::test]
async fn count_command_comparison_produces_checked_single_meaning_but_cannot_use_physical20_inventory_codec()
 {
    use harvestcircle_domain::hub::{
        actuals::{
            ActualKnownCountExplanation, ActualKnownInterpretation, ActualKnownInterpretations,
        },
        corrections::{
            CorrectedDepartureFamily, CountLossQuantityAccount, OriginalCountLossQuantity,
            OriginalRecipientAttribution,
        },
    };
    let (fixture, db, query) = world().await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    let review = db
        .hub_count_explanation_review(owner(), operation(5), query.clone(), at(150))
        .await
        .unwrap();
    let HubCountExplanationReviewOutcome::Candidate(plan) = review.outcome else {
        panic!("checked comparison");
    };
    let original = plan.known_event;
    let account = CountLossQuantityAccount::restore(
        OriginalCountLossQuantity {
            reconciliation: plan.count.id,
            operation: plan.count.operation,
            stock_revision: plan.count.stock_revision,
            trace: original.trace,
            variant: original.variant.id,
            packs: packs(2),
        },
        vec![plan.explanation],
    )
    .unwrap();
    let record = ActualKnownInterpretation {
        stock_revision: original.stock_revision,
        catalog_revision: original.catalog_revision,
        latest_recorded_at: original.recorded_at,
        recipient_original: OriginalRecipientAttribution {
            hub: original.hub,
            family: CorrectedDepartureFamily::ExplainedCountLoss,
            operation: original.operation,
            stock_revision: original.stock_revision,
            catalog_revision: original.catalog_revision,
            recorded_at: original.recorded_at,
            actor: original.actor,
            recipient: original.recipient.clone(),
            populations: vec![original.trace],
            issued_versions: vec![],
        },
        original,
        recipient_history: vec![],
        quantities: vec![],
        count_explanation: Some(std::sync::Arc::new(ActualKnownCountExplanation {
            count: plan.count,
            account,
        })),
    };
    assert_eq!(record.view().unwrap().effective_packs, packs(1));
    assert!(serde_json::to_vec(&record).is_ok());
    let set = ActualKnownInterpretations {
        hub: record.original.hub,
        stock_revision: record.stock_revision,
        catalog_revision: record.catalog_revision,
        latest_recorded_at: record.latest_recorded_at,
        records: vec![record],
    };
    assert!(
        serde_json::to_vec(&set)
            .unwrap_err()
            .to_string()
            .contains("shared account capture")
    );
    // Explicit write-only schema2 is distinct even for an empty inventory.
    let mut empty = set.clone();
    empty.records.clear();
    let empty = harvestcircle_domain::hub::actuals::ActualKnownCountInventory::new(empty).unwrap();
    let empty_json = serde_json::to_value(&empty).unwrap();
    assert_eq!(empty_json["schema"], 2);
    assert_eq!(empty_json["count_accounts"].as_array().unwrap().len(), 0);
    assert_eq!(empty_json["records"].as_array().unwrap().len(), 0);
    let inventory =
        harvestcircle_domain::hub::actuals::ActualKnownCountInventory::new(set).unwrap();
    let value = serde_json::to_value(&inventory).unwrap();
    assert_eq!(value["schema"], 2);
    assert_eq!(value["count_accounts"].as_array().unwrap().len(), 1);
    assert_eq!(value["records"].as_array().unwrap().len(), 1);
    assert_eq!(
        value["records"][0]["count_account"],
        value["count_accounts"][0]["operation"]
    );
    assert!(value["records"][0].get("count_explanation").is_none());
    assert_ne!(empty.digest().unwrap(), inventory.digest().unwrap());
    assert_count_inventory_archive_bound(&inventory);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

fn assert_count_inventory_archive_bound(
    inventory: &harvestcircle_domain::hub::actuals::ActualKnownCountInventory,
) {
    use harvestcircle_application::hub_submission::{
        HUB_SUBMISSION_ARCHIVE_MAX_BYTES, HubRuntimeIncarnation, HubSubmissionContext,
        HubSubmittedKnownCountInventory, HubSubmittedMaterial, HubSubmittedRequest,
    };
    use harvestcircle_domain::hub::{
        HubOwnership,
        actuals::ActualKnownCountInventory,
        corrections::{
            CorrectedDepartureFamily, DepartureQuantityAccount, OriginalDepartureQuantity,
        },
        reference::{ReferenceCommand, ReferenceMutation, ReferenceValue},
    };
    fn archived(
        inventory: ActualKnownCountInventory,
    ) -> Result<
        harvestcircle_application::hub_submission::ArchivedHubSubmission,
        harvestcircle_application::hub_submission::HubRequestArchiveError,
    > {
        let facts = inventory.facts();
        let hub = HubOwnership::new(facts.hub, owner());
        let catalog_revision = facts.catalog_revision;
        let stock_revision = facts.stock_revision;
        let request = ReferenceCommand {
            expected_catalog_revision: catalog_revision,
            mutation: ReferenceMutation::Create {
                id: id(250),
                value: ReferenceValue::Product {
                    label: HubLabel::new("Encoding only; no native execution grant").unwrap(),
                },
            },
        };
        let material = [HubSubmittedMaterial::KnownCountInventory(Box::new(
            HubSubmittedKnownCountInventory {
                id: id(251),
                inventory,
            },
        ))];
        HubSubmittedRequest::Reference(&request).capture(
            &operation(902),
            HubSubmissionScope {
                runtime: HubRuntimeIncarnation::from_bytes([0x73; 16]).unwrap(),
                generation: SessionGeneration::initial(),
                actor: owner(),
            },
            &HubSubmissionContext {
                hub,
                catalog_revision: Some(catalog_revision),
                stock_revision: Some(stock_revision),
                material: &material,
            },
        )
    }
    let capture = archived(inventory.clone()).unwrap();
    assert!(capture.as_bytes().len() < HUB_SUBMISSION_ARCHIVE_MAX_BYTES);
    let encoded: serde_json::Value = serde_json::from_slice(capture.as_bytes()).unwrap();
    let counted = &encoded["material"][0]["known_count_inventory"]["inventory"];
    assert_eq!(counted["schema"], 2);
    assert_eq!(counted["count_accounts"].as_array().unwrap().len(), 1);
    // Typed maximum-history input, not native posting acceptance. Every event
    // refers to one shared account; the actual application writer emits it once.
    use harvestcircle_domain::hub::corrections::{CountExplanationAmendment, CountLossExplanation};
    let mut maximum = inventory.facts().clone();
    let sample = maximum.records[0].clone();
    let snapshot = sample.count_explanation.as_ref().unwrap();
    let number = harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS / 2;
    fn history_id(n: u64) -> HubRecordId {
        let mut bytes = [0xC7; 16];
        bytes[8..].copy_from_slice(&n.to_be_bytes());
        HubRecordId::from_bytes(bytes).unwrap()
    }
    let explanations: Vec<_> = (0..number)
        .map(|i| CountLossExplanation {
            operation: history_id((2 * i + 1) as u64),
            stock_revision: HubRevision::from_value((2 * i + 6) as u64).unwrap(),
            packs: packs(1),
        })
        .collect();
    let amendments = explanations
        .iter()
        .enumerate()
        .map(|(i, row)| CountExplanationAmendment {
            operation: history_id((2 * i + 2) as u64),
            stock_revision: HubRevision::from_value(row.stock_revision.value() + 1).unwrap(),
            explanation: row.operation,
            before: packs(1),
            after: packs(0),
        })
        .collect();
    let shared = std::sync::Arc::new(
        harvestcircle_domain::hub::actuals::ActualKnownCountExplanation {
            count: snapshot.count.clone(),
            account: CountLossQuantityAccount::restore_corrected(
                snapshot.account.original(),
                explanations.clone(),
                amendments,
                vec![],
            )
            .unwrap(),
        },
    );
    maximum.stock_revision = HubRevision::from_value(10_000).unwrap();
    maximum.records = explanations
        .iter()
        .map(|row| {
            let mut record = sample.clone();
            record.stock_revision = maximum.stock_revision;
            record.original.operation = row.operation;
            record.original.stock_revision = row.stock_revision;
            record.recipient_original.operation = row.operation;
            record.recipient_original.stock_revision = row.stock_revision;
            record.count_explanation = Some(std::sync::Arc::clone(&shared));
            record
        })
        .collect();
    let maximum = ActualKnownCountInventory::new(maximum).unwrap();
    let maximum = archived(maximum).unwrap();
    assert!(maximum.as_bytes().len() < HUB_SUBMISSION_ARCHIVE_MAX_BYTES);
    let maximum: serde_json::Value = serde_json::from_slice(maximum.as_bytes()).unwrap();
    let maximum = &maximum["material"][0]["known_count_inventory"]["inventory"];
    assert_eq!(maximum["count_accounts"].as_array().unwrap().len(), 1);
    assert_eq!(maximum["records"].as_array().unwrap().len(), number);
    assert_eq!(
        maximum["count_accounts"][0]["account"]["explanations"]
            .as_array()
            .unwrap()
            .len(),
        number
    );
    assert_eq!(
        maximum["count_accounts"][0]["account"]["amendments"]
            .as_array()
            .unwrap()
            .len(),
        number
    );

    // Pure descriptive capacity data, never a legally posted native world. The
    // real application writer must reject its oversized body without SQL effects.
    let mut oversized = inventory.facts().clone();
    let template = &oversized.records[0];
    let mut records = Vec::new();
    for index in 0..20_000_u64 {
        let mut row = template.clone();
        let mut bytes = [0xB7; 16];
        bytes[8..].copy_from_slice(&(index + 1).to_be_bytes());
        let own = HubRecordId::from_bytes(bytes).unwrap();
        let revision = HubRevision::from_value(index + 2).unwrap();
        row.original.operation = own;
        row.original.stock_revision = revision;
        row.recipient_original.operation = own;
        row.recipient_original.stock_revision = revision;
        row.recipient_original.family = CorrectedDepartureFamily::ReviewedActual;
        row.count_explanation = None;
        row.quantities = vec![
            DepartureQuantityAccount::restore(
                OriginalDepartureQuantity {
                    operation: own,
                    group: row.original.lines[0].group,
                    trace: row.original.trace,
                    variant: row.original.variant.id,
                    stock_revision: revision,
                    packs: row.original.lines[0].packs,
                },
                vec![],
                packs(0),
            )
            .unwrap(),
        ];
        row.stock_revision = HubRevision::from_value(25_000).unwrap();
        records.push(row);
    }
    oversized.stock_revision = HubRevision::from_value(25_000).unwrap();
    oversized.records = records;
    let oversized = ActualKnownCountInventory::new(oversized).unwrap();
    assert_eq!(
        archived(oversized).unwrap_err(),
        harvestcircle_application::hub_submission::HubRequestArchiveError::Capacity
    );
}
