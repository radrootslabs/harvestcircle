mod composition;
mod faults;
mod stability;
use super::*;
use crate::hub::stock::tests::{Fixture, id, operation, owner, query};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::hub::{
    corrections::{AmendedRecipient, CorrectionImpact},
    departure::DepartureCommand,
    reference::SourceProvenance,
    stock::Occurrence,
};
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Reviewed whole original and amended event; no physical change").unwrap()
}
async fn command(
    db: &Database,
    target: HubRecordId,
    recipient: Option<HubRecordId>,
    prior: Option<HubRecordId>,
) -> RecipientAttributionCommand {
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    RecipientAttributionCommand {
        departure_operation: target,
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_prior: prior,
        recipient,
        basis: note(),
    }
}
async fn stock_state(fixture: &Fixture) -> (i64, i64) {
    use sqlx::Connection;
    let mut raw = fixture.raw().await;
    let state = sqlx::query_as("SELECT revision,event_count FROM hub_stock_state")
        .fetch_one(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    state
}

#[tokio::test]
async fn original_a_unknown_then_b_keep_whole_event_paper_and_physical_truth_after_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let paper = crate::hub::handling::departure::tests::issued_departure_fixture(&db).await;
    let current = db
        .hub_dispatch_record(owner(), paper.record.id)
        .await
        .unwrap();
    let departed = db
        .apply_hub_departure(
            owner(),
            operation(30),
            DepartureCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                preparation: paper.record.id,
                expected_preparation_revision: paper.record.revision,
                identification: paper.record.versions[0].id,
                occurred: Occurrence::Unknown,
                observation: note(),
            },
            at(110),
        )
        .await
        .unwrap();
    let before = db.hub_stock_page(owner(), query()).await.unwrap();
    let original_paper = db
        .hub_dispatch_operation(owner(), operation(21))
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
    let physical_before = crate::hub::custody::tests::material(&fixture).await;
    let state_before = stock_state(&fixture).await;
    let target = crate::hub::actuals::common::operation_id(&operation(30)).unwrap();
    let input = command(&db, target, None, None).await;
    let review = db
        .hub_recipient_attribution_review(owner(), operation(1900), input.clone(), at(200))
        .await
        .unwrap();
    let crate::HubAttributionReviewOutcome::Candidate(plan) = review.outcome else {
        panic!("recipient")
    };
    let crate::HubMutationResult::RecipientAttributed(first) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::RecipientAttribution(Box::new(input.clone())),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    assert_eq!(first.original, *plan);
    assert!(matches!(first.original.after, AmendedRecipient::Unknown));
    assert!(
        first
            .original
            .impact
            .contains(CorrectionImpact::IssuedVersion(paper.record.versions[0].id))
    );
    assert!(
        first
            .original
            .impact
            .contains(CorrectionImpact::Recipient(id(8)))
    );
    assert!(
        first
            .original
            .impact
            .contains(CorrectionImpact::UnresolvedRecipient(target))
    );
    let second = command(
        &db,
        target,
        Some(id(9)),
        Some(first.original.stamp.operation),
    )
    .await;
    let crate::HubMutationResult::RecipientAttributed(second) = db
        .submit_hub_mutation(
            scope,
            operation(1901),
            HubMutation::RecipientAttribution(Box::new(second)),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    assert!(matches!(second.original.after,AmendedRecipient::Known(ref p) if p.id==id(9)));
    assert!(
        second
            .original
            .impact
            .contains(CorrectionImpact::Recipient(id(8)))
    );
    assert!(
        second
            .original
            .impact
            .contains(CorrectionImpact::Recipient(id(9)))
    );
    assert!(
        second
            .original
            .impact
            .contains(CorrectionImpact::UnresolvedRecipient(target))
    );
    assert_eq!(
        db.hub_stock_page(owner(), query()).await.unwrap().rows,
        before.rows
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(departed.clone())
    );
    assert_eq!(
        db.hub_dispatch_operation(owner(), operation(21))
            .await
            .unwrap(),
        original_paper
    );
    let physical = |rows: Vec<(String, Vec<String>)>| {
        rows.into_iter()
            .filter(|(name, _)| {
                !matches!(
                    name.as_str(),
                    "hub_stock_operations"
                        | "hub_stock_state"
                        | "hub_submissions"
                        | "hub_corrections"
                        | "hub_correction_scope_links"
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        physical(physical_before),
        physical(crate::hub::custody::tests::material(&fixture).await)
    );
    assert_eq!(
        stock_state(&fixture).await,
        (state_before.0 + 2, state_before.1)
    );
    let crate::HubMutationResult::RecipientAttributed(replay) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::RecipientAttribution(Box::new(input)),
            at(220),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    assert_eq!(replay, first);
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let source_command = harvestcircle_domain::hub::corrections::SourceAttributionCommand {
        trace: paper.record.versions[0].material.lines[0].trace.id,
        expected_stock_revision: page.revision,
        expected_catalog_revision: page.catalog_revision,
        expected_prior: None,
        source: harvestcircle_domain::hub::reference::SourceAttribution::Referenced {
            source: harvestcircle_domain::hub::reference::SourceReference {
                id: id(90),
                issuer: None,
                kind: harvestcircle_domain::hub::reference::SourceReferenceKind::Other,
                code: harvestcircle_domain::hub::reference::SourceCode::new(
                    "Fictional reviewed source",
                )
                .unwrap(),
                provenance: note(),
            },
            certainty: harvestcircle_domain::hub::reference::SourceCertainty::ReportedPossible,
        },
        basis: note(),
    };
    let crate::HubMutationResult::SourceAttributed(source) = db
        .submit_hub_mutation(
            scope,
            operation(1902),
            HubMutation::SourceAttribution(Box::new(source_command)),
            at(230),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    for impact in [
        CorrectionImpact::Recipient(id(8)),
        CorrectionImpact::Recipient(id(9)),
        CorrectionImpact::UnresolvedRecipient(target),
        CorrectionImpact::IssuedVersion(paper.record.versions[0].id),
    ] {
        assert!(source.original.impact.contains(impact));
    }

    let trace = paper.record.versions[0].material.lines[0].trace.id;
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
    assert_eq!(
        snapshot.recipient_corrections,
        vec![first.original.clone(), second.original.clone()]
    );
    assert!(
        matches!(snapshot.current_recipient(target).unwrap(),AmendedRecipient::Known(ref p) if p.id==id(9))
    );
    assert_eq!(snapshot.exits[0].departure.recipient.id, id(8));
    let mut corrupt_snapshot = snapshot.clone();
    corrupt_snapshot.recipient_corrections[0]
        .original
        .populations
        .clear();
    assert_eq!(
        corrupt_snapshot
            .includes_source_history(&harvestcircle_domain::hub::trace::TraceSelection::default()),
        Err(harvestcircle_domain::hub::trace::TraceError::InvalidSnapshot)
    );
    let history = db
        .hub_trace_history(
            owner(),
            crate::HubTraceHistoryQuery {
                trace,
                expected_revision: Some(snapshot.stock_revision),
                expected_catalog_revision: Some(snapshot.catalog_revision),
                offset: 0,
                limit: 64,
            },
        )
        .await
        .unwrap();
    let recipient_rows = history
        .rows
        .iter()
        .filter(|row| row.kind == crate::HubTraceHistoryKind::RecipientAttributed)
        .collect::<Vec<_>>();
    assert_eq!(recipient_rows.len(), 2);
    assert!(
        recipient_rows
            .iter()
            .all(|row| row.occurred.is_none() && row.observation == note())
    );
    let output = harvestcircle_application::hub_output::trace_output(&snapshot).unwrap();
    for label in [
        "Before recipient amendment",
        "After recipient amendment",
        "Recipient A",
        "Recipient B",
        "Unknown recipient",
        "Retained affected scope",
    ] {
        assert!(output.html.contains(label), "{label}");
    }
    for text in ["Recipient A", "Recipient B", "Unknown recipient"] {
        let mut query = crate::HubTraceQuery::default();
        query.selection.text = text.into();
        let page = db.hub_trace_page(owner(), query).await.unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].trace.id, trace);
        assert_eq!(page.rows[0].recipient_corrections.len(), 2);
    }
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1900))
            .await
            .unwrap()
            .as_ref(),
        Some(first.as_ref())
    );
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1901))
            .await
            .unwrap()
            .as_ref(),
        Some(second.as_ref())
    );
    assert_eq!(
        db.hub_departure_operation(owner(), operation(30))
            .await
            .unwrap(),
        Some(departed)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn recipient_of_actual_keeps_old_actual_and_link_recovery_raw_ingress_and_fresh_review_fence()
{
    use harvestcircle_domain::{
        SafeErrorCode,
        hub::actuals::{
            ActualDuplicateLink, ActualNewDepartureCommand, ActualReportCommand,
            ActualReportSources,
        },
    };
    let (fixture, db, additional) = crate::hub::corrections::additional_review_fixture().await;
    let target = additional.facts.target_operation;
    let target_op = crate::hub::actuals::common::operation(target).unwrap();
    let actual = db
        .hub_actual_review_operation(owner(), target_op.clone())
        .await
        .unwrap()
        .unwrap();
    let crate::HubActualReviewOriginal::Departure(original_plan) = &actual.original else {
        panic!("actual")
    };
    let trace = original_plan.reduction.before.trace;
    let variant = original_plan.reduction.before.variant;
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
    let raw = |recipient| ActualReportCommand {
        variant: Some(variant),
        recipient: Some(recipient),
        packs: Some(harvestcircle_domain::hub::reference::WholePacks::from_value(2).unwrap()),
        sources: ActualReportSources::Unknown,
        occurred: Occurrence::Unknown,
        observation: note(),
    };
    let crate::HubMutationResult::ActualReported(old_report) = db
        .submit_hub_mutation(
            scope,
            operation(1850),
            HubMutation::ActualReport(Box::new(raw(id(3)))),
            at(190),
        )
        .await
        .unwrap()
    else {
        panic!("report")
    };
    let old_link = ActualDuplicateLink {
        report: old_report.original.id,
        known_operation: target,
        known_trace: trace,
        known_variant: variant,
        verification: note(),
    };
    let crate::HubMutationResult::ActualReviewed(link) = db
        .submit_hub_mutation(
            scope,
            operation(1851),
            HubMutation::ActualLink(Box::new(old_link.clone())),
            at(191),
        )
        .await
        .unwrap()
    else {
        panic!("link")
    };
    let invalid_target = crate::hub::actuals::common::operation_id(&operation(1851)).unwrap();
    let invalid = command(&db, invalid_target, Some(id(4)), None).await;
    let before_invalid = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_recipient_attribution_review(owner(), operation(1890), invalid.clone(), at(240))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1890),
            HubMutation::RecipientAttribution(Box::new(invalid)),
            at(240)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubInvalidInput
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        before_invalid
    );
    let input = command(&db, target, Some(id(4)), None).await;
    let crate::HubMutationResult::RecipientAttributed(recipient) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::RecipientAttribution(Box::new(input)),
            at(250),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    assert!(matches!(recipient.original.after,AmendedRecipient::Known(ref r) if r.id==id(4)));
    let known = db
        .hub_correction_known_departures(
            owner(),
            crate::HubCorrectionKnownQuery {
                trace,
                expected_stock_revision: None,
                expected_catalog_revision: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(known.departures.len(), 1);
    assert_eq!(
        known.departures[0].recipient_history,
        vec![recipient.original.clone()]
    );
    assert_eq!(known.departures[0].original.recipient.id, id(3));
    let crate::HubMutationResult::ActualReviewed(replay) = db
        .submit_hub_mutation(
            scope,
            operation(1851),
            HubMutation::ActualLink(Box::new(old_link)),
            at(260),
        )
        .await
        .unwrap()
    else {
        panic!("old link replay")
    };
    assert_eq!(link, replay);
    assert_eq!(
        db.hub_actual_review_operation(owner(), target_op.clone())
            .await
            .unwrap(),
        Some(actual.clone())
    );
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(1950),
            HubMutation::ActualReport(Box::new(raw(id(4)))),
            at(270),
        )
        .await
        .unwrap()
    else {
        panic!("preserved raw ingress")
    };
    let group = db
        .hub_group(owner(), additional.balancing_group)
        .await
        .unwrap();
    let mut facts = original_plan.comparison.facts.clone();
    facts.report = report.original.id;
    facts.expected_stock_revision = group.revision;
    facts.expected_catalog_revision = group.catalog_revision;
    facts.expected_group_revision = group.group.revision;
    facts.group = group.group.id;
    facts.recipient = id(4);
    let new_review = ActualNewDepartureCommand {
        facts,
        source_operation: additional.source_operation,
        source_group: additional.source_group,
        duplicate_review_basis: note(),
    };
    let new_link = ActualDuplicateLink {
        report: report.original.id,
        known_operation: target,
        known_trace: trace,
        known_variant: variant,
        verification: note(),
    };
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_departure_preview(owner(), operation(1951), new_review.clone(), at(280))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        db.hub_actual_link_preview(owner(), operation(1952), new_link.clone(), at(280))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::HubConflict
    );
    for (operation, mutation) in [
        (
            operation(1951),
            HubMutation::ActualReview(Box::new(new_review)),
        ),
        (operation(1952), HubMutation::ActualLink(Box::new(new_link))),
    ] {
        assert_eq!(
            db.submit_hub_mutation(scope, operation, mutation, at(280))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubConflict
        );
    }
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    assert_eq!(
        db.hub_actual_report_operation(owner(), operation(1950))
            .await
            .unwrap()
            .as_ref(),
        Some(report.as_ref())
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_actual_review_operation(owner(), target_op)
            .await
            .unwrap(),
        Some(actual)
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), operation(1851))
            .await
            .unwrap()
            .as_ref(),
        Some(link.as_ref())
    );
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1900))
            .await
            .unwrap()
            .as_ref(),
        Some(recipient.as_ref())
    );
    db.close().await.unwrap();
}

async fn world() -> (
    Fixture,
    Database,
    HubSubmissionScope,
    RecipientAttributionCommand,
) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    crate::hub::handling::departure::tests::multi_population_departure(&db).await;
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
    let input = command(
        &db,
        crate::hub::actuals::common::operation_id(&operation(30)).unwrap(),
        None,
        None,
    )
    .await;
    (
        fixture,
        db,
        HubSubmissionScope {
            runtime,
            generation,
            actor: owner(),
        },
        input,
    )
}
