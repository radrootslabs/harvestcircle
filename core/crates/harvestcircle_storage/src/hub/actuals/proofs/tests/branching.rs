//! Genuine native history and default-stack reconstruction, without fake roots.
use super::*;
use crate::hub::stock::tests::id;
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::{
    UnixTimestamp,
    hub::{
        actuals::{
            ActualNewDepartureCommand, ActualReportCommand, ActualReportSources, ActualReviewFacts,
        },
        reference::{HubRecordId, SourceProvenance, WholePacks},
        stock::Occurrence,
    },
};

fn at(seconds: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(seconds).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new(
        "Independently verified fictional departure at its distinct original time",
    )
    .unwrap()
}

#[tokio::test]
async fn default_stack_branching_originals_share_proofs_and_small_cache_rebuilds_preserve_every_result_after_reopen()
 {
    let (fixture, db, hub, trace) = world().await;
    let first = db
        .hub_actual_review_operation(owner(), operation(81))
        .await
        .unwrap()
        .unwrap();
    let HubActualReviewOriginal::Departure(plan) = &first.original else {
        panic!("physical")
    };
    let group = plan.reduction.before.id;
    let runtime = db.begin_hub_runtime(at(290)).await.unwrap();
    let generation = db
        .advance_hub_session(
            runtime,
            SessionGeneration::initial(),
            Some(owner()),
            at(290),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let mut originals = vec![first];
    for n in 0..16u32 {
        let occurred = Occurrence::Known(at(111 + i64::from(n)));
        let crate::HubMutationResult::ActualReported(report) = db
            .submit_hub_mutation(
                scope,
                operation(4000 + 2 * n),
                HubMutation::ActualReport(Box::new(ActualReportCommand {
                    variant: None,
                    recipient: None,
                    packs: Some(WholePacks::from_value(1).unwrap()),
                    sources: ActualReportSources::Unknown,
                    occurred,
                    observation: note(),
                })),
                at(300 + 10 * i64::from(n)),
            )
            .await
            .unwrap()
        else {
            panic!("report")
        };
        let view = db.hub_group(owner(), group).await.unwrap();
        let crate::HubMutationResult::ActualReviewed(review) = db
            .submit_hub_mutation(
                scope,
                operation(4001 + 2 * n),
                HubMutation::ActualReview(Box::new(ActualNewDepartureCommand {
                    facts: ActualReviewFacts {
                        report: report.original.id,
                        expected_stock_revision: view.revision,
                        expected_catalog_revision: view.catalog_revision,
                        group,
                        expected_group_revision: view.group.revision,
                        packs: WholePacks::from_value(1).unwrap(),
                        recipient: id(3),
                        occurred,
                        source_evidence: note(),
                        quantity_evidence: note(),
                        recipient_evidence: note(),
                        timing_evidence: note(),
                    },
                    source_operation: HubRecordId::from_hex(
                        &operation(1).as_str().replace('-', ""),
                    )
                    .unwrap(),
                    source_group: group,
                    duplicate_review_basis: note(),
                })),
                at(305 + 10 * i64::from(n)),
            )
            .await
            .unwrap()
        else {
            panic!("review")
        };
        originals.push(*review);
    }
    assert_eq!(
        db.hub_group(owner(), group)
            .await
            .unwrap()
            .group
            .packs
            .value(),
        2
    );
    let expected = db
        .hub_actual_known_interpretations(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap()
        .interpretations;
    assert_eq!(expected.records.len(), 17);
    let (stock, catalog) = (expected.stock_revision, expected.catalog_revision);
    let trace_query = crate::HubTraceSnapshotQuery {
        trace,
        expected_revision: None,
        expected_catalog_revision: None,
    };
    let history_query = crate::HubTraceHistoryQuery {
        trace,
        expected_revision: None,
        expected_catalog_revision: None,
        offset: 0,
        limit: 64,
    };
    let trace_snapshot = db
        .hub_trace_snapshot(owner(), trace_query.clone())
        .await
        .unwrap();
    assert_eq!(trace_snapshot.actual_departures.len(), 17);
    let trace_history = db
        .hub_trace_history(owner(), history_query.clone())
        .await
        .unwrap();
    assert_eq!(
        trace_history
            .rows
            .iter()
            .filter(|r| r.kind == crate::HubTraceHistoryKind::ActualReconciled)
            .count(),
        17
    );
    let before = crate::hub::custody::tests::material(&fixture).await;
    db.host().transaction(|tx| Box::pin(async move {
        {
            let mut reader = OriginalReader::new(tx);
            for _ in 0..2 {
                assert_eq!(Box::pin(corrections::actual_interpretations_at_with_proofs_on(&mut reader, hub, trace, stock, catalog)).await?, expected);
            }
            let counts = reader.proof_counts();
            assert_eq!(counts.0, 17);
            assert!(counts.1 >= 34 && counts.2 == 17 && counts.3 <= PAYLOAD_BYTES);
            println!("Default seventeen-original phase rebuild/hit/entry/charged-byte counts: {counts:?}");
        }
        // Only the private test cache is constrained; native capacities and
        // every original outcome remain exactly the same.
        let mut reader = OriginalReader::for_test_budget(tx, 2, 64*1024);
        assert_eq!(Box::pin(corrections::actual_interpretations_at_with_proofs_on(&mut reader, hub, trace, stock, catalog)).await?, expected);
        let counts = reader.proof_counts();
        assert!(counts.0 > 17 && counts.0 <= 3*17);
        assert!(counts.2 <= 2 && counts.3 <= 64*1024);
        println!("Constrained seventeen-original phase rebuild/hit/entry/charged-byte counts: {counts:?}");
        Ok::<_, SafeError>(())
    })).await.unwrap();
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_trace_snapshot(owner(), trace_query).await.unwrap(),
        trace_snapshot
    );
    assert_eq!(
        db.hub_trace_history(owner(), history_query).await.unwrap(),
        trace_history
    );
    for receipt in originals {
        assert_eq!(
            db.hub_actual_review_operation(owner(), receipt.operation.clone())
                .await
                .unwrap(),
            Some(receipt)
        );
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
