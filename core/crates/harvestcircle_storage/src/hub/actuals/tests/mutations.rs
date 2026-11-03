//! Real native admission and atomic effects on fresh fictional schema18 fixtures.
use super::*;
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::hub::actuals::{
    ActualNewDepartureCommand, ActualReportCommand, ActualReportSources, ActualReviewFacts,
};

pub(super) async fn scope(db: &Database) -> HubSubmissionScope {
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
pub(super) fn report() -> ActualReportCommand {
    ActualReportCommand {
        variant: None,
        recipient: None,
        packs: Some(packs(2)),
        sources: ActualReportSources::Unknown,
        occurred: Occurrence::Unknown,
        observation: note(
            "Two packs already departed; source and timing need factual verification",
        ),
    }
}
pub(super) async fn review(
    db: &Database,
    report: HubRecordId,
    group: HubRecordId,
) -> ActualNewDepartureCommand {
    let view = db.hub_group(owner(), group).await.unwrap();
    ActualNewDepartureCommand {
        facts: ActualReviewFacts {
            report,
            expected_stock_revision: view.revision,
            expected_catalog_revision: view.catalog_revision,
            group,
            expected_group_revision: view.group.revision,
            packs: packs(2),
            recipient: id(3),
            occurred: Occurrence::Known(at(110)),
            source_evidence: note("Original observed receiving cohort identified"),
            quantity_evidence: note("Two whole packs physically verified"),
            recipient_evidence: note("Actual recipient independently confirmed"),
            timing_evidence: note("Observed departure before report recorded"),
        },
        source_operation: HubRecordId::from_hex(&operation(1).as_str().replace('-', "")).unwrap(),
        source_group: group,
        duplicate_review_basis: note(
            "Complete original departure history reviewed; no matching prior event",
        ),
    }
}

#[tokio::test]
async fn exported_refined_actual_keeps_original_cross_clock_interval_and_recording_time() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let scope = scope(&db).await;
    let mut input = report();
    input.occurred = Occurrence::Interval {
        earliest: at(140),
        latest: at(160),
    };
    let crate::HubMutationResult::ActualReported(saved) = db
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
    let mut command = review(&db, saved.original.id, intake.original.position.group).await;
    command.facts.occurred = Occurrence::Known(at(145));
    db.submit_hub_mutation(
        scope,
        operation(81),
        HubMutation::ActualReview(Box::new(command)),
        at(170),
    )
    .await
    .unwrap();
    let snapshot = db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: intake.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    let plan = &snapshot.actual_departures[0];
    assert_eq!(plan.report, saved.original);
    assert_eq!(plan.report.recorded_at, at(150));
    assert_eq!(plan.comparison.facts.occurred, Occurrence::Known(at(145)));
    let html = harvestcircle_application::hub_output::trace_output(&snapshot)
        .unwrap()
        .html;
    assert!(html.contains("Original reported occurrence (UTC)"));
    assert!(html.contains("Reviewed physical occurrence (UTC)"));
    assert!(html.contains("00:02:20") && html.contains("00:02:40") && html.contains("00:02:25"));
    assert!(html.contains("Recording clock conflict; original claim retained"));
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(80))
            .await
            .unwrap(),
        Some(*saved)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn report_preserves_future_claim_and_replay_without_stock_effect() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let scope = scope(&db).await;
    let before = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let mut command = report();
    command.occurred = Occurrence::Known(at(160));
    let result = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(command.clone())),
            at(150),
        )
        .await
        .unwrap();
    let crate::HubMutationResult::ActualReported(saved) = result else {
        panic!("report original")
    };
    assert_eq!(saved.original.report, command);
    assert_eq!(saved.original.recorded_at, at(150));
    assert_eq!(
        saved.original.recording_clock_assessment(),
        harvestcircle_domain::hub::actuals::ActualReportedClockAssessment::RecordingClockConflict
    );
    let after = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    assert_eq!(after.group, before.group);
    assert_eq!(after.revision.value(), before.revision.value() + 1);
    let page = db
        .hub_actual_reports(owner(), HubActualReportQuery::default())
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].report, *saved);
    assert!(page.rows[0].review_operation.is_none());
    let replay = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(command)),
            at(170),
        )
        .await
        .unwrap();
    let crate::HubMutationResult::ActualReported(replay) = replay else {
        panic!("exact report replay")
    };
    assert_eq!(replay, saved);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(80))
            .await
            .unwrap(),
        Some(*saved)
    );
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap(),
        after
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn reviewed_actual_reduces_stock_and_pending_work_atomically_and_keeps_originals() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let scope = scope(&db).await;
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
        panic!("report")
    };
    let command = review(&db, saved.original.id, intake.original.position.group).await;
    let crate::HubMutationResult::ActualReviewed(reviewed) = db
        .submit_hub_mutation(
            scope,
            operation(81),
            HubMutation::ActualReview(Box::new(command.clone())),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("review")
    };
    let HubActualReviewOriginal::Departure(plan) = &reviewed.original else {
        panic!("physical original")
    };
    assert_eq!(plan.reduction.before.packs, packs(20));
    assert_eq!(plan.reduction.on_hand_after, packs(18));
    assert_eq!(plan.reduction.work_changes.len(), 1);
    let after = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    assert_eq!(after.group.packs, packs(18));
    let quantity = after.group.work[0].requirement.quantity;
    assert_eq!(quantity.required(), packs(20));
    assert_eq!(quantity.completed(), packs(0));
    assert_eq!(quantity.no_longer_required(), packs(2));
    assert_eq!(quantity.remaining(), packs(18));
    assert!(
        db.hub_actual_reports(owner(), HubActualReportQuery::default())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    let evidence = db
        .hub_actual_source(
            owner(),
            HubActualSourceQuery {
                context: query(intake.original.trace.id),
                operation: operation(1),
                group: intake.original.position.group,
            },
        )
        .await
        .unwrap();
    assert_eq!(evidence.source.group.packs, packs(20));
    assert_eq!(evidence.lineage.lineage.nodes()[0].packs, packs(18));
    assert_eq!(evidence.known_departures.len(), 1);
    assert_eq!(evidence.known_departures[0].packs().unwrap(), packs(2));
    assert_eq!(evidence.subsequent_activity.len(), 1);
    let crate::HubMutationResult::ActualReviewed(replay) = db
        .submit_hub_mutation(
            scope,
            operation(81),
            HubMutation::ActualReview(Box::new(command)),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("review replay")
    };
    assert_eq!(replay, reviewed);
    let snapshot = db
        .hub_trace_snapshot(
            owner(),
            crate::HubTraceSnapshotQuery {
                trace: intake.original.trace.id,
                expected_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(snapshot.balance().unwrap().actual_departed, packs(2));
    assert_eq!(snapshot.balance().unwrap().departed, packs(0));
    assert_eq!(snapshot.actual_departures.len(), 1);
    let output = harvestcircle_application::hub_output::trace_output(&snapshot).unwrap();
    assert!(output.html.contains("Reconciled already-taken departures"));
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(81))
            .await
            .unwrap(),
        Some(*reviewed)
    );
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap(),
        after
    );
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(80))
            .await
            .unwrap(),
        Some(*saved)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn duplicate_link_retains_known_actual_original_without_second_stock_or_work_effect() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let scope = scope(&db).await;
    let crate::HubMutationResult::ActualReported(first) = db
        .submit_hub_mutation(
            scope,
            operation(80),
            HubMutation::ActualReport(Box::new(report())),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("report")
    };
    let command = review(&db, first.original.id, intake.original.position.group).await;
    let crate::HubMutationResult::ActualReviewed(physical) = db
        .submit_hub_mutation(
            scope,
            operation(81),
            HubMutation::ActualReview(Box::new(command)),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("physical")
    };
    let crate::HubMutationResult::ActualReported(duplicate) = db
        .submit_hub_mutation(
            scope,
            operation(82),
            HubMutation::ActualReport(Box::new(report())),
            at(170),
        )
        .await
        .unwrap()
    else {
        panic!("duplicate report")
    };
    let before = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let link = harvestcircle_domain::hub::actuals::ActualDuplicateLink {
        report: duplicate.original.id,
        known_operation: HubRecordId::from_hex(&operation(81).as_str().replace('-', "")).unwrap(),
        known_trace: intake.original.trace.id,
        known_variant: id(2),
        verification: note("This is the same original physical event, not another departure"),
    };
    let crate::HubMutationResult::ActualReviewed(linked) = db
        .submit_hub_mutation(
            scope,
            operation(83),
            HubMutation::ActualLink(Box::new(link.clone())),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("link")
    };
    let HubActualReviewOriginal::Duplicate(original) = &linked.original else {
        panic!("duplicate original")
    };
    assert_eq!(original.original, duplicate.original);
    let HubActualReviewOriginal::Departure(physical) = &physical.original else {
        panic!("physical original")
    };
    assert_eq!(original.known, physical.physical_projection().unwrap());
    assert!(linked.lineage.is_none());
    assert!(linked.historical_holds.is_empty());
    let after = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    assert_eq!(before.group, after.group);
    assert_eq!(after.revision.value(), before.revision.value() + 1);
    assert!(
        db.hub_actual_reports(owner(), HubActualReportQuery::default())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    let evidence = db
        .hub_actual_source(
            owner(),
            HubActualSourceQuery {
                context: query(intake.original.trace.id),
                operation: operation(1),
                group: intake.original.position.group,
            },
        )
        .await
        .unwrap();
    assert_eq!(evidence.known_departures.len(), 1);
    assert_eq!(evidence.subsequent_activity.len(), 1);
    let crate::HubMutationResult::ActualReviewed(replay) = db
        .submit_hub_mutation(
            scope,
            operation(83),
            HubMutation::ActualLink(Box::new(link)),
            at(190),
        )
        .await
        .unwrap()
    else {
        panic!("link replay")
    };
    assert_eq!(replay, linked);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(83))
            .await
            .unwrap(),
        Some(*linked)
    );
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap(),
        after
    );
    db.close().await.unwrap();
}
