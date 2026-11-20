//! Fresh current producer fixtures; never open or recreate a retained20 world.
mod branches;
mod guard_faults;
mod maximum;
mod proofs;
use super::*;
use crate::hub::{
    handling::trace::{HubTraceQuery, HubTraceSnapshotQuery},
    stock::tests::Fixture,
};
use harvestcircle_domain::hub::{
    corrections::{CorrectionObservationCommand, CorrectionObservedFacts},
    reference::{SourceProvenance, WholePacks},
    trace::{TraceScope, TraceSelection},
};

fn at(value: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(value).unwrap()
}
fn id(value: u32) -> HubRecordId {
    crate::hub::actuals::common::operation_id(&operation(value)).unwrap()
}
async fn classify(
    db: &Database,
    scope: HubSubmissionScope,
    trace: HubRecordId,
    explanation: HubRecordId,
    account_prior: HubRecordId,
    packs: u32,
    step: (u32, i64),
) -> Box<crate::hub::corrections::HubCountClassificationReceipt> {
    let crate::HubMutationResult::CorrectionObserved(observation) = db
        .submit_hub_mutation(
            scope,
            operation(step.0),
            HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                previous: None,
                facts: CorrectionObservedFacts::CountClassification {
                    explanation_operation: explanation,
                    corrected_packs: WholePacks::from_value(packs).unwrap(),
                    report: None,
                    basis: SourceProvenance::new(
                        "Independent corrected interpretation of this exact explanation",
                    )
                    .unwrap(),
                },
            })),
            at(step.1),
        )
        .await
        .unwrap()
    else {
        panic!("raw classification observation")
    };
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
    let crate::HubMutationResult::CountClassified(result) = db
        .submit_hub_mutation(
            scope,
            operation(step.0 + 1),
            HubMutation::CountClassification(Box::new(CountClassificationCommand {
                observation_operation: observation.original.operation,
                expected_stock_revision: current.stock_revision,
                expected_catalog_revision: current.catalog_revision,
                expected_account_prior: Some(account_prior),
                expected_prior: None,
            })),
            at(step.1 + 10),
        )
        .await
        .unwrap()
    else {
        panic!("classification")
    };
    result
}

async fn physical(fixture: &Fixture) -> Vec<(String, Vec<String>)> {
    crate::hub::custody::tests::material(fixture)
        .await
        .into_iter()
        .filter(|(table, _)| is_physical_table(table))
        .collect()
}

fn is_physical_table(table: &str) -> bool {
    !matches!(
        table,
        "hub_submissions"
            | "hub_stock_operations"
            | "hub_stock_state"
            | "hub_corrections"
            | "hub_correction_scope_links"
            | "hub_correction_captures"
            | "hub_correction_count_explanations"
            | "hub_correction_quantity_changes"
            | "hub_count_interpretation_markers"
            | "hub_actual_report_reviews"
            | "hub_correction_observations"
    )
}

#[tokio::test]
async fn first_explanation_zero_classification_and_original_replay_keep_all_physical_rows_after_reopen()
 {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 22);
    let (fixture, db, command) =
        super::super::super::count_review::tests::world_with_recipient(5).await;
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    let before = physical(&fixture).await;
    let review = db
        .hub_count_explanation_review(owner(), operation(801), command.clone(), at(200))
        .await
        .expect("native22 read-only explanation review");
    assert!(matches!(
        review.outcome,
        crate::hub::corrections::HubCountExplanationReviewOutcome::Candidate(_)
    ));
    db.capture_hub_submission(
        owner(),
        scope,
        operation(801),
        HubMutation::CountExplanation(Box::new(command.clone())),
    )
    .await
    .expect("native22 descriptive explanation capture");
    assert_eq!(physical(&fixture).await, before);
    let crate::HubMutationResult::CountExplained(first) = db
        .submit_hub_mutation(
            scope,
            operation(801),
            HubMutation::CountExplanation(Box::new(command.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("count explanation")
    };
    assert_eq!(physical(&fixture).await, before);
    let trace = first.original.known_event.trace;
    let snapshot = db
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
    let balance = snapshot.balance().unwrap();
    assert_eq!(
        (
            balance.on_hand.value(),
            balance.counted_loss.value(),
            balance.actual_departed.value()
        ),
        (18, 1, 1)
    );
    let crate::HubMutationResult::CorrectionObserved(observed) = db
        .submit_hub_mutation(
            scope,
            operation(802),
            HubMutation::CorrectionObservation(Box::new(CorrectionObservationCommand {
                previous: None,
                facts: CorrectionObservedFacts::CountClassification {
                    explanation_operation: id(801),
                    corrected_packs: WholePacks::from_value(0).unwrap(),
                    report: None,
                    basis: SourceProvenance::new(
                        "Independent fictional zero classification; original shortage remains",
                    )
                    .unwrap(),
                },
            })),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("classification observation")
    };
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
    let crate::HubMutationResult::CountClassified(classified) = db
        .submit_hub_mutation(
            scope,
            operation(803),
            HubMutation::CountClassification(Box::new(CountClassificationCommand {
                observation_operation: observed.original.operation,
                expected_stock_revision: current.stock_revision,
                expected_catalog_revision: current.catalog_revision,
                expected_account_prior: Some(id(801)),
                expected_prior: None,
            })),
            at(220),
        )
        .await
        .unwrap()
    else {
        panic!("zero classification")
    };
    assert_eq!(physical(&fixture).await, before);
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
    let balance = current.balance().unwrap();
    assert_eq!(
        (
            balance.on_hand.value(),
            balance.counted_loss.value(),
            balance.actual_departed.value()
        ),
        (18, 2, 0)
    );
    let inventory = current.count_interpretations.as_ref().unwrap();
    assert_eq!(inventory.facts().records.len(), 1);
    assert_eq!(inventory.views()[0].effective_packs.value(), 0);
    let selection = TraceSelection {
        scope: TraceScope::All,
        text: first.original.known_event.recipient.id.to_hex(),
    };
    assert!(!selection.includes(&current.trace).unwrap());
    let page = db
        .hub_trace_page(
            owner(),
            HubTraceQuery {
                selection,
                ..HubTraceQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].trace.id, trace);
    let catalog = db.hub_references(owner()).await.unwrap();
    db.submit_hub_mutation(
        scope,
        operation(804),
        HubMutation::Reference(Box::new(
            harvestcircle_domain::hub::reference::ReferenceCommand {
                expected_catalog_revision: catalog.revision(),
                mutation: harvestcircle_domain::hub::reference::ReferenceMutation::Revise {
                    id: command.facts.recipient,
                    expected_record_revision: first.original.known_event.recipient.revision,
                    value: harvestcircle_domain::hub::reference::ReferenceValue::Party {
                        label: harvestcircle_domain::hub::reference::HubLabel::new(
                            "Later current recipient label",
                        )
                        .unwrap(),
                    },
                },
            },
        )),
        at(225),
    )
    .await
    .unwrap();
    let after_rename = physical(&fixture).await;
    let crate::HubMutationResult::CountExplained(replay) = db
        .submit_hub_mutation(
            scope,
            operation(801),
            HubMutation::CountExplanation(Box::new(command)),
            at(230),
        )
        .await
        .unwrap()
    else {
        panic!("exact replay")
    };
    assert_eq!(replay, first);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_count_explanation_operation(owner(), operation(801))
            .await
            .unwrap(),
        Some(*first)
    );
    assert_eq!(
        db.hub_count_classification_operation(owner(), operation(803))
            .await
            .unwrap(),
        Some(*classified)
    );
    assert_eq!(physical(&fixture).await, after_rename);
    db.close().await.unwrap();
}

#[tokio::test]
async fn interleaved_explanations_share_one_shortage_account_and_keep_zero_originals_after_reopen()
{
    use harvestcircle_domain::hub::{
        actuals::{ActualReportCommand, ActualReportSources, ReportedActualSource},
        stock::Occurrence,
    };
    let (fixture, db, command) =
        super::super::super::count_review::tests::world_with_recipient(5).await;
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    let before = physical(&fixture).await;
    let crate::HubMutationResult::CountExplained(first) = db
        .submit_hub_mutation(
            scope,
            operation(801),
            HubMutation::CountExplanation(Box::new(command.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("first explanation")
    };
    let trace = first.original.known_event.trace;
    let zero = classify(&db, scope, trace, id(801), id(801), 0, (900, 210)).await;
    assert_eq!(physical(&fixture).await, before);
    let crate::HubMutationResult::ActualReported(report) = db.submit_hub_mutation(scope, operation(804), HubMutation::ActualReport(Box::new(ActualReportCommand {
        variant: Some(first.original.known_event.variant.id), recipient: Some(command.facts.recipient), packs: Some(WholePacks::from_value(2).unwrap()),
        sources: ActualReportSources::EstablishedClaim(ReportedActualSource { trace, group: Some(command.source_group), location: Some(first.original.count.references.location.id) }),
        occurred: Occurrence::Known(at(80)), observation: SourceProvenance::new("Independent two-pack actual fact after zero classification; no additional physical debit").unwrap(),
    })), at(230)).await.unwrap() else { panic!("second raw report") };
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
    let before_second = physical(&fixture).await;
    let mut second_command = command;
    second_command.facts.report = report.original.id;
    second_command.facts.packs = WholePacks::from_value(2).unwrap();
    second_command.facts.occurred = Occurrence::Known(at(80));
    second_command.facts.expected_stock_revision = current.stock_revision;
    second_command.facts.expected_catalog_revision = current.catalog_revision;
    second_command.facts.expected_prior = Some(id(901));
    let crate::HubMutationResult::CountExplained(second) = db
        .submit_hub_mutation(
            scope,
            operation(805),
            HubMutation::CountExplanation(Box::new(second_command)),
            at(240),
        )
        .await
        .unwrap()
    else {
        panic!("second explanation")
    };
    let amended = classify(&db, scope, trace, id(805), id(805), 1, (910, 250)).await;
    assert_eq!(physical(&fixture).await, before_second);
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
    assert_eq!(
        inventory
            .views()
            .iter()
            .map(|view| view.effective_packs.value())
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    let records = &inventory.facts().records;
    assert!(std::sync::Arc::ptr_eq(
        records[0].count_explanation.as_ref().unwrap(),
        records[1].count_explanation.as_ref().unwrap()
    ));
    assert_eq!(records[0].original.packs().unwrap().value(), 1);
    assert_eq!(records[1].original.packs().unwrap().value(), 2);
    let balance = current.balance().unwrap();
    assert_eq!(
        (
            balance.on_hand.value(),
            balance.counted_loss.value(),
            balance.actual_departed.value()
        ),
        (18, 1, 1)
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    for original in [*first, *second] {
        assert_eq!(
            db.hub_count_explanation_operation(owner(), original.operation.clone())
                .await
                .unwrap(),
            Some(original)
        );
    }
    for original in [*zero, *amended] {
        assert_eq!(
            db.hub_count_classification_operation(owner(), original.operation.clone())
                .await
                .unwrap(),
            Some(original)
        );
    }
    assert_eq!(physical(&fixture).await, before_second);
    db.close().await.unwrap();
}

#[tokio::test]
async fn sixteen_interleaved_explanations_keep_one_account_and_cold_originals_on_default_stack() {
    qualify_interleaved_explanations(16, true, 1).await;
}

#[tokio::test]
#[ignore = "Explicit wider original count-cut precursor for the distinct explanation maximum"]
async fn sixteen_distinct_explanations_with_wide_original_count_cut_reopen_cold() {
    qualify_interleaved_explanations(16, true, 1000).await;
}

#[tokio::test]
#[ignore = "Explicit ordinary native2048E+2048C shared capacity and cold full-prefix qualification"]
async fn native_2048_distinct_explanations_and_classifications_keep_complete_account_reopen_cold() {
    qualify_interleaved_explanations(2048, false, 1000).await;
}

fn qualify_interleaved_explanations(
    steps: u32,
    read_all: bool,
    time_scale: i64,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        use harvestcircle_domain::hub::{
            actuals::{ActualReportCommand, ActualReportSources, ReportedActualSource},
            stock::Occurrence,
        };
        let (fixture, db, template) =
            super::super::super::count_review::tests::world_with_recipient_clock(5, time_scale)
                .await;
        let runtime = db.hub_runtime_session().await.unwrap().unwrap();
        let scope = HubSubmissionScope {
            runtime: runtime.runtime,
            generation: runtime.generation,
            actor: runtime.actor.unwrap(),
        };
        let physical_only = |rows: Vec<(String, Vec<String>)>| {
            rows.into_iter()
                .filter(|(table, _)| {
                    !matches!(
                        table.as_str(),
                        "hub_actual_reports" | "hub_actual_report_sources"
                    )
                })
                .collect::<Vec<_>>()
        };
        let before = physical_only(physical(&fixture).await);
        let mut prior = None;
        let mut explanations = Vec::new();
        let mut classifications = Vec::new();
        let mut trace = None;
        for step in 0..steps {
            let operation_base = 10_000 + step * 10;
            let time = 200 * time_scale + i64::from(step) * 40;
            let occurred = Occurrence::Known(at(70 * time_scale + i64::from(step) * 2));
            let mut command = template.clone();
            command.facts.occurred = occurred;
            command.facts.expected_prior = prior;
            if step > 0 {
                let crate::HubMutationResult::ActualReported(report) = db.submit_hub_mutation(scope, operation(operation_base), HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(crate::hub::stock::tests::id(2)), recipient: Some(template.facts.recipient), packs: Some(WholePacks::from_value(1).unwrap()),
                sources: ActualReportSources::EstablishedClaim(ReportedActualSource { trace: trace.unwrap(), group: Some(template.source_group), location: Some(crate::hub::stock::tests::id(6)) }),
                occurred, observation: SourceProvenance::new("Distinct independently observed occurrence; prior zero originals remain retained").unwrap(),
            })), at(time-10)).await.unwrap() else { panic!("independent raw fact") };
                command.facts.report = report.original.id;
            }
            let page = db
                .hub_stock_page(owner(), crate::hub::stock::tests::query())
                .await
                .unwrap();
            command.facts.expected_stock_revision = page.revision;
            command.facts.expected_catalog_revision = page.catalog_revision;
            let crate::HubMutationResult::CountExplained(explanation) = db
                .submit_hub_mutation(
                    scope,
                    operation(operation_base + 1),
                    HubMutation::CountExplanation(Box::new(command)),
                    at(time),
                )
                .await
                .unwrap()
            else {
                panic!("bounded explanation")
            };
            trace = Some(explanation.original.known_event.trace);
            let classification = classify(
                &db,
                scope,
                trace.unwrap(),
                id(operation_base + 1),
                id(operation_base + 1),
                0,
                (operation_base + 2, time + 10),
            )
            .await;
            prior = Some(id(operation_base + 3));
            // The maximum validates the complete account in the cold last
            // original, but retains only boundary receipts as API oracles.
            // Keep every receipt and public lookup in the original16 case.
            if read_all || step == 0 || step + 1 == steps {
                explanations.push(explanation);
                classifications.push(classification);
            }
            if (step + 1) % 16 == 0 {
                eprintln!(
                    "ordinary native distinct Count account committed{} explanations and classifications",
                    step + 1
                );
            }
        }
        let trace = trace.unwrap();
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
        assert_eq!(inventory.facts().records.len(), steps as usize);
        assert!(
            inventory
                .views()
                .iter()
                .all(|view| view.effective_packs.value() == 0)
        );
        let shared = inventory.facts().records[0]
            .count_explanation
            .as_ref()
            .unwrap();
        assert!(
            inventory
                .facts()
                .records
                .iter()
                .all(|record| std::sync::Arc::ptr_eq(
                    shared,
                    record.count_explanation.as_ref().unwrap()
                ))
        );
        assert_eq!(shared.account.explanations().len(), steps as usize);
        assert_eq!(shared.account.amendments().len(), steps as usize);
        let balance = current.balance().unwrap();
        assert_eq!(
            (
                balance.on_hand.value(),
                balance.counted_loss.value(),
                balance.actual_departed.value()
            ),
            (18, 2, 0)
        );
        assert_eq!(physical_only(physical(&fixture).await), before);
        db.close().await.unwrap();
        let db = fixture.open().await;
        // The latest classification is deliberately the first original read in this
        // reopened phase; no earlier lookup warms its predecessor certificates.
        let last = classifications.last().unwrap();
        assert_eq!(
            db.hub_count_classification_operation(owner(), last.operation.clone())
                .await
                .unwrap()
                .as_ref(),
            Some(last.as_ref())
        );
        for original in explanations {
            assert_eq!(
                db.hub_count_explanation_operation(owner(), original.operation.clone())
                    .await
                    .unwrap(),
                Some(*original)
            );
        }
        for original in classifications {
            assert_eq!(
                db.hub_count_classification_operation(owner(), original.operation.clone())
                    .await
                    .unwrap(),
                Some(*original)
            );
        }
        assert_eq!(physical_only(physical(&fixture).await), before);
        db.close().await.unwrap();
    })
}
