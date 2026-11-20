use super::*;
mod history;
use harvestcircle_domain::hub::{
    actuals::{ActualReportCommand, ActualReportSources, ReportedActualSource},
    corrections::RecipientAttributionCommand,
    counts::{CountObserveCommand, CountPostCommand},
    stock::Occurrence,
};

#[tokio::test]
async fn two_native_count_accounts_with_interleaved_recipient_and_zero_history_reopen_cold() {
    let (fixture, db, template) =
        super::super::super::super::count_review::tests::world_with_recipient(5).await;
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    // Close the initial report before another physical count. This is ordinary
    // native admission, preserving the unresolved-actual guard on CountPosting.
    let crate::HubMutationResult::CountExplained(seed) = db
        .submit_hub_mutation(
            scope,
            operation(801),
            HubMutation::CountExplanation(Box::new(template.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("first account explanation")
    };
    let seed_classification = classify(
        &db,
        scope,
        seed.original.known_event.trace,
        id(801),
        id(801),
        0,
        (802, 210),
    )
    .await;
    let seed_recipient = attribute_unknown(&db, scope, id(801), 804, 230).await;
    let catalog = db.hub_references(owner()).await.unwrap();
    let mut receipt = crate::hub::stock::tests::command(&catalog);
    receipt.expected_stock_revision = db
        .hub_stock_page(owner(), crate::hub::stock::tests::query())
        .await
        .unwrap()
        .revision;
    receipt.occurred = Occurrence::Known(at(120));
    let crate::HubMutationResult::Received(second_intake) = db
        .submit_hub_mutation(
            scope,
            operation(9005),
            HubMutation::Receive(Box::new(receipt)),
            at(250),
        )
        .await
        .unwrap()
    else {
        panic!("independent second native receipt")
    };
    let group = db
        .hub_group(owner(), second_intake.original.position.group)
        .await
        .unwrap();
    let crate::HubMutationResult::CountObserved(observation) = db
        .submit_hub_mutation(
            scope,
            operation(9000),
            HubMutation::CountObservation(Box::new(CountObserveCommand {
                expected_stock_revision: group.revision,
                expected_catalog_revision: group.catalog_revision,
                group: group.group.id,
                expected_group_revision: group.group.revision,
                counted_packs: WholePacks::from_value(19).unwrap(),
                cutoff: Occurrence::Known(at(260)),
                observation: SourceProvenance::new("Independent second native count").unwrap(),
            })),
            at(270),
        )
        .await
        .unwrap()
    else {
        panic!("second native count observation")
    };
    let reviewed = db
        .hub_count_review(owner(), observation.original.id)
        .await
        .unwrap();
    let crate::HubMutationResult::CountPosted(second) = db
        .submit_hub_mutation(
            scope,
            operation(9001),
            HubMutation::CountPosting(Box::new(CountPostCommand {
                expected_stock_revision: reviewed.stock_revision,
                expected_catalog_revision: reviewed.catalog_revision,
                count: observation.original.id,
                reviewed_activity: true,
                surplus_identification: None,
                reason: SourceProvenance::new("Second distinct shortage account").unwrap(),
            })),
            at(280),
        )
        .await
        .unwrap()
    else {
        panic!("second native count posting")
    };
    assert_ne!(template.facts.count_reconciliation, second.original.id);
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
    let mut heads = [Some(id(803)), None];
    let traces = [
        seed.original.known_event.trace,
        second_intake.original.trace.id,
    ];
    let groups = [template.source_group, second_intake.original.position.group];
    let mut explanations = vec![seed];
    let mut classifications = vec![seed_classification];
    let mut recipients = vec![seed_recipient];
    for step in 1..8_u32 {
        let account = (step % 2) as usize;
        let base = 20000 + step * 10;
        let time = 400 + i64::from(step) * 60;
        let occurred = Occurrence::Known(at(
            if account == 0 { 70 } else { 140 } + i64::from(step / 2) * 2
        ));
        let mut command = template.clone();
        command.facts.count_reconciliation = if account == 0 {
            template.facts.count_reconciliation
        } else {
            second.original.id
        };
        command.facts.expected_prior = heads[account];
        command.facts.occurred = occurred;
        command.source_group = groups[account];
        if account == 1 {
            command.source_operation = id(9005);
        }
        if step > 0 {
            let crate::HubMutationResult::ActualReported(report) = db
                .submit_hub_mutation(
                    scope,
                    operation(base),
                    HubMutation::ActualReport(Box::new(ActualReportCommand {
                        variant: Some(crate::hub::stock::tests::id(2)),
                        recipient: Some(template.facts.recipient),
                        packs: Some(WholePacks::from_value(1).unwrap()),
                        sources: ActualReportSources::EstablishedClaim(ReportedActualSource {
                            trace: traces[account],
                            group: Some(groups[account]),
                            location: Some(crate::hub::stock::tests::id(6)),
                        }),
                        occurred,
                        observation: SourceProvenance::new(
                            "Independent occurrence for its exact counted shortage",
                        )
                        .unwrap(),
                    })),
                    at(time - 10),
                )
                .await
                .unwrap()
            else {
                panic!("raw independent account report")
            };
            command.facts.report = report.original.id;
        }
        let page = db
            .hub_stock_page(owner(), crate::hub::stock::tests::query())
            .await
            .unwrap();
        command.facts.expected_stock_revision = page.revision;
        command.facts.expected_catalog_revision = page.catalog_revision;
        let review = db
            .hub_count_explanation_review(owner(), operation(base + 1), command.clone(), at(time))
            .await
            .unwrap();
        assert!(matches!(review.outcome, super::super::super::super::count_review::HubCountExplanationReviewOutcome::Candidate(_)), "account step {step}: {:?}", review.outcome);
        let crate::HubMutationResult::CountExplained(explanation) = db
            .submit_hub_mutation(
                scope,
                operation(base + 1),
                HubMutation::CountExplanation(Box::new(command)),
                at(time),
            )
            .await
            .unwrap()
        else {
            panic!("interleaved account explanation")
        };
        let classification = classify(
            &db,
            scope,
            traces[account],
            id(base + 1),
            id(base + 1),
            0,
            (base + 2, time + 10),
        )
        .await;
        heads[account] = Some(id(base + 3));
        let recipient = attribute_unknown(&db, scope, id(base + 1), base + 4, time + 30).await;
        explanations.push(explanation);
        classifications.push(classification);
        recipients.push(recipient);
    }
    assert_eq!(physical_only(physical(&fixture).await), before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    let last = recipients.last().unwrap();
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), last.operation.clone())
            .await
            .unwrap()
            .as_ref(),
        Some(last.as_ref())
    );
    let last = classifications.last().unwrap();
    assert_eq!(
        db.hub_count_classification_operation(owner(), last.operation.clone())
            .await
            .unwrap()
            .as_ref(),
        Some(last.as_ref())
    );
    let last = explanations.last().unwrap();
    assert_eq!(
        db.hub_count_explanation_operation(owner(), last.operation.clone())
            .await
            .unwrap()
            .as_ref(),
        Some(last.as_ref())
    );
    for (trace, reconciliation, on_hand, shortage) in [
        (traces[0], template.facts.count_reconciliation, 18, 2),
        (traces[1], second.original.id, 19, 1),
    ] {
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
        assert_eq!(inventory.facts().records.len(), 4);
        for record in &inventory.facts().records {
            assert_eq!(
                record.recipient_original.recipient.id,
                crate::hub::stock::tests::id(5)
            );
            assert_eq!(record.recipient_history.len(), 1);
            assert!(matches!(
                record.recipient_history[0].after,
                harvestcircle_domain::hub::corrections::AmendedRecipient::Unknown
            ));
        }
        assert!(
            inventory
                .views()
                .iter()
                .all(|view| view.effective_packs.value() == 0)
        );
        {
            let records = inventory
                .facts()
                .records
                .iter()
                .filter(|record| {
                    record.count_explanation.as_ref().unwrap().count.id == reconciliation
                })
                .collect::<Vec<_>>();
            assert_eq!(records.len(), 4);
            let account = records[0].count_explanation.as_ref().unwrap();
            assert_eq!(
                account.account.original().packs.value(),
                if reconciliation == template.facts.count_reconciliation {
                    2
                } else {
                    1
                }
            );
            assert!(records.iter().all(|record| std::sync::Arc::ptr_eq(
                account,
                record.count_explanation.as_ref().unwrap()
            )));
            assert_eq!(account.account.explanations().len(), 4);
            assert_eq!(account.account.amendments().len(), 4);
        }
        let balance = current.balance().unwrap();
        assert_eq!(
            (
                balance.on_hand.value(),
                balance.counted_loss.value(),
                balance.actual_departed.value()
            ),
            (on_hand, shortage, 0)
        );
    }
    assert_eq!(physical_only(physical(&fixture).await), before);
    db.close().await.unwrap();
    history::qualify_foreign_suffix(
        &fixture,
        explanations[0].clone(),
        Box::new(second.original.clone()),
        classifications[0].original.amendment.stock_revision,
    )
    .await;
}

async fn attribute_unknown(
    db: &Database,
    scope: HubSubmissionScope,
    target: HubRecordId,
    operation_number: u32,
    time: i64,
) -> Box<super::super::super::super::recipient_ledger::HubRecipientAttributionReceipt> {
    let page = db
        .hub_stock_page(owner(), crate::hub::stock::tests::query())
        .await
        .unwrap();
    let crate::HubMutationResult::RecipientAttributed(receipt) = db
        .submit_hub_mutation(
            scope,
            operation(operation_number),
            HubMutation::RecipientAttribution(Box::new(RecipientAttributionCommand {
                departure_operation: target,
                expected_stock_revision: page.revision,
                expected_catalog_revision: page.catalog_revision,
                expected_prior: None,
                recipient: None,
                basis: SourceProvenance::new(
                    "Retained zero original now has explicitly unknown recipient",
                )
                .unwrap(),
            })),
            at(time),
        )
        .await
        .unwrap()
    else {
        panic!("interleaved recipient correction")
    };
    receipt
}
