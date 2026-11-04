use super::*;
use crate::hub::stock::tests::{Fixture, command as receiving, id, operation, owner, query, setup};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::hub::reference::{
    SourceCertainty, SourceCode, SourceProvenance, SourceReference,
};
mod documents;
mod faults;
mod historical_prefix;
mod stability;

async fn world() -> (
    Fixture,
    Database,
    HubSubmissionScope,
    SourceAttributionCommand,
) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receiving(&catalog), at(100))
        .await
        .unwrap();
    let runtime = db.begin_hub_runtime(at(150)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(150),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let input = command(&db, receipt.original.trace.id, source(), None).await;
    (fixture, db, scope, input)
}

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn source() -> SourceAttribution {
    SourceAttribution::Referenced {
        source: SourceReference {
            id: id(90),
            issuer: Some(id(3)),
            kind: SourceReferenceKind::SupplierReference,
            code: SourceCode::new("Fictional independently corrected source").unwrap(),
            provenance: SourceProvenance::new(
                "Reviewed actual source attribution; no physical change",
            )
            .unwrap(),
        },
        certainty: SourceCertainty::Established,
    }
}
async fn command(
    db: &Database,
    trace: HubRecordId,
    source: SourceAttribution,
    prior: Option<HubRecordId>,
) -> SourceAttributionCommand {
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    SourceAttributionCommand {
        trace,
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_prior: prior,
        source,
        basis: SourceProvenance::new("Reviewed complete original and current associations")
            .unwrap(),
    }
}
#[tokio::test]
async fn source_posting_preserves_original_physical_work_and_recovers_chain_after_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receiving(&catalog), at(100))
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let runtime = db.begin_hub_runtime(at(150)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(150),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let input = command(&db, receipt.original.trace.id, source(), None).await;
    let result = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::SourceAttribution(Box::new(input.clone())),
            at(200),
        )
        .await
        .unwrap();
    let crate::HubMutationResult::SourceAttributed(first) = result else {
        panic!("source")
    };
    assert_eq!(first.original.before.source, SourceAttribution::Unknown);
    assert_eq!(first.original.after.source, source());
    let after = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(after.rows, before.rows);
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(receipt.clone())
    );
    let second_input = command(
        &db,
        receipt.original.trace.id,
        SourceAttribution::Unknown,
        Some(first.original.stamp.operation),
    )
    .await;
    let result = db
        .submit_hub_mutation(
            scope,
            operation(1901),
            HubMutation::SourceAttribution(Box::new(second_input)),
            at(210),
        )
        .await
        .unwrap();
    let crate::HubMutationResult::SourceAttributed(second) = result else {
        panic!("source")
    };
    assert_eq!(second.original.before.source, source());
    assert_eq!(second.original.after.source, SourceAttribution::Unknown);
    let replay = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::SourceAttribution(Box::new(input)),
            at(220),
        )
        .await
        .unwrap();
    let crate::HubMutationResult::SourceAttributed(replayed) = replay else {
        panic!("source")
    };
    assert_eq!(replayed, first);
    let current = command(
        &db,
        receipt.original.trace.id,
        source(),
        Some(second.original.stamp.operation),
    )
    .await;
    let review = db
        .hub_source_attribution_review(owner(), operation(1902), current.clone(), at(220))
        .await
        .unwrap();
    assert_eq!(
        review.history,
        vec![first.original.clone(), second.original.clone()]
    );
    assert_eq!(review.known_sources, vec![source()]);
    assert!(matches!(
        review.outcome,
        crate::HubAttributionReviewOutcome::Candidate(_)
    ));
    let snapshot = db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: receipt.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(snapshot.trace, receipt.original.trace);
    assert_eq!(
        snapshot.current_source().unwrap(),
        &SourceAttribution::Unknown
    );
    assert_eq!(snapshot.source_corrections, review.history);
    let document = harvestcircle_application::hub_output::trace_output(&snapshot).unwrap();
    assert!(
        document
            .html
            .contains("Linked source corrections: original and current meaning")
    );
    assert!(
        document
            .html
            .contains("Fictional independently corrected source")
    );
    assert!(document.html.contains("Unresolved source for population"));
    assert!(!document.html.contains("HubRecordId(["));
    let page = db
        .hub_trace_page(
            owner(),
            crate::HubTraceQuery {
                selection: harvestcircle_domain::hub::trace::TraceSelection {
                    text: "independently corrected".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].trace.id, receipt.original.trace.id);
    assert_eq!(page.rows[0].source_corrections, review.history);
    let history = db
        .hub_trace_history(
            owner(),
            crate::HubTraceHistoryQuery {
                trace: receipt.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        history
            .rows
            .iter()
            .filter(|r| r.kind == crate::HubTraceHistoryKind::SourceAttributed)
            .count(),
        2
    );
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*first)
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1901))
            .await
            .unwrap(),
        Some(*second)
    );
    assert_eq!(
        db.hub_source_attribution_review(owner(), operation(1902), current, at(220))
            .await
            .unwrap(),
        review
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn source_then_additional_and_found_keep_original_source_cut_and_complete_follow_up_without_recursion()
 {
    use harvestcircle_domain::hub::corrections::{CorrectionImpact, PhysicalDebitKey};
    let (fixture, db, mut additional) = crate::hub::corrections::additional_review_fixture().await;
    let target = crate::hub::actuals::common::operation(additional.facts.target_operation).unwrap();
    let balancing = additional.balancing_group;
    let trace = db.hub_group(owner(), balancing).await.unwrap().group.trace;
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
    let source_input = command(&db, trace, source(), None).await;
    let crate::HubMutationResult::SourceAttributed(source_posted) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::SourceAttribution(Box::new(source_input)),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    assert!(
        source_posted
            .original
            .impact
            .contains(CorrectionImpact::Departure(
                additional.facts.target_operation
            ))
    );
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    additional.facts.expected_stock_revision = page.revision;
    let target_group = additional.facts.target_group;
    let crate::HubMutationResult::AdditionalCorrected(posted) = db
        .submit_hub_mutation(
            scope,
            operation(1901),
            HubMutation::AdditionalQuantity(Box::new(additional)),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("additional")
    };
    assert!(posted.impact.contains(CorrectionImpact::Source(id(90))));
    assert!(
        posted
            .impact
            .contains(CorrectionImpact::UnresolvedSource(trace))
    );
    let found = super::super::additional_write_tests::found(
        &db,
        scope,
        &target,
        target_group,
        PhysicalDebitKey {
            operation: posted.original.operation,
            group: balancing,
        },
        1950,
        220,
    )
    .await;
    assert!(found.impact.contains(CorrectionImpact::Source(id(90))));
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*source_posted.clone())
    );
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
    assert_eq!(snapshot.current_source().unwrap(), &source());
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*source_posted)
    );
    assert_eq!(
        db.hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace,
                expected_revision: None,
                expected_catalog_revision: None
            }
        )
        .await
        .unwrap(),
        snapshot
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn fully_identified_original_and_amended_source_searches_keep_independent_bounded_fields() {
    use harvestcircle_domain::hub::{
        stock::ReceiptIdentities,
        trace::{TraceScope, TraceSelection},
    };
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let catalog = setup(&db).await;
    let mut receiving = receiving(&catalog);
    let original_id = ReceiptIdentities::from_operation_bytes(
        crate::hub::actuals::common::operation_id(&operation(1)).unwrap(),
    )
    .unwrap()
    .source;
    receiving.source = SourceAttribution::Referenced {
        source: SourceReference {
            id: original_id,
            issuer: Some(id(4)),
            kind: SourceReferenceKind::ProducerBatch,
            code: SourceCode::new("ORIGINAL-NATIVE-SOURCE").unwrap(),
            provenance: SourceProvenance::new(
                "Fully identified fictional original; eight search fields",
            )
            .unwrap(),
        },
        certainty: SourceCertainty::Established,
    };
    let receipt = db
        .apply_hub_receipt(owner(), operation(1), receiving, at(100))
        .await
        .unwrap();
    let runtime = db.begin_hub_runtime(at(150)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(150),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let input = command(&db, receipt.original.trace.id, source(), None).await;
    db.submit_hub_mutation(
        scope,
        operation(1900),
        HubMutation::SourceAttribution(Box::new(input)),
        at(200),
    )
    .await
    .unwrap();
    let selections = vec![
        TraceSelection::default(),
        TraceSelection {
            text: "ORIGINAL-NATIVE-SOURCE".into(),
            ..Default::default()
        },
        TraceSelection {
            text: "independently corrected".into(),
            ..Default::default()
        },
        TraceSelection {
            scope: TraceScope::SourceImpact(original_id),
            ..Default::default()
        },
        TraceSelection {
            scope: TraceScope::SourceImpact(id(90)),
            ..Default::default()
        },
    ];
    for selection in &selections {
        let page = db
            .hub_trace_page(
                owner(),
                crate::HubTraceQuery {
                    selection: selection.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].trace.id, receipt.original.trace.id);
    }
    db.close().await.unwrap();
    let db = fixture.open().await;
    for selection in selections {
        let page = db
            .hub_trace_page(
                owner(),
                crate::HubTraceQuery {
                    selection,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, 1);
    }
    db.close().await.unwrap();
}
