use super::*;
use crate::hub::actuals::proofs::OriginalReader;
use crate::hub::corrections::{count_context, count_review::HubCountExplanationQuery};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        actuals::{ActualReportCommand, ActualReportSources},
        reference::{HubLabel, ReferenceCommand, ReferenceMutation, ReferenceValue},
        stock::Occurrence,
    },
};
use sqlx::Connection;

fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::from_seconds(n).unwrap()
}
fn note() -> SourceProvenance {
    SourceProvenance::new("Independent fictional original-cut fact evidence").unwrap()
}
async fn facts(
    db: &Database,
    query: HubCountExplanationQuery,
) -> Result<count_context::Facts, SafeError> {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                let mut reader = OriginalReader::new(tx);
                Box::pin(count_context::facts_on(&mut reader, hub, &query)).await
            })
        })
        .await
        .map_err(map_transaction_error)
}
async fn marker(
    db: &Database,
    report: HubRecordId,
    cut: HubRevision,
) -> Result<Option<HubRecordId>, SafeError> {
    db.host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?.id();
                let mut reader = OriginalReader::new(tx);
                count_context::report_marker_through_on(&mut reader, hub, report, cut).await
            })
        })
        .await
        .map_err(map_transaction_error)
}

#[tokio::test]
async fn historical_count_facts_keep_paired_catalog_clock_and_inventory_without_clearing_live_pending()
 {
    let (fixture, db, query) = crate::hub::corrections::count_review::tests::world().await;
    let original = facts(&db, query.clone()).await.unwrap();
    let runtime = db.hub_runtime_session().await.unwrap().unwrap();
    let scope = HubSubmissionScope {
        runtime: runtime.runtime,
        generation: runtime.generation,
        actor: runtime.actor.unwrap(),
    };
    let party = original.catalog.get(query.facts.recipient).unwrap().clone();
    let crate::HubMutationResult::Reference(changed) = db
        .submit_hub_mutation(
            scope,
            operation(25),
            HubMutation::Reference(Box::new(ReferenceCommand {
                expected_catalog_revision: query.facts.expected_catalog_revision,
                mutation: ReferenceMutation::Revise {
                    id: party.id,
                    expected_record_revision: party.revision,
                    value: ReferenceValue::Party {
                        label: HubLabel::new("Later current recipient description").unwrap(),
                    },
                },
            })),
            at(150),
        )
        .await
        .unwrap()
    else {
        panic!("reference")
    };
    let raw = ActualReportCommand {
        variant: None,
        recipient: None,
        packs: None,
        sources: ActualReportSources::Unknown,
        occurred: Occurrence::Unknown,
        observation: note(),
    };
    db.submit_hub_mutation(
        scope,
        operation(30),
        HubMutation::ActualReport(Box::new(raw.clone())),
        at(170),
    )
    .await
    .unwrap();
    let decision = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(40),
                    HubMutation::ActualReport(Box::new(raw)),
                    at(180),
                )
                .await
            })
        })
        .await
        .map_err(map_transaction_error)
        .unwrap();
    drop(decision);
    let pending = db
        .hub_submission(owner(), operation(40))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    let before = crate::hub::custody::tests::material(&fixture).await;
    let restored = facts(&db, query.clone()).await.unwrap();
    assert_eq!(restored.stock_revision, original.stock_revision);
    assert_eq!(restored.catalog, original.catalog);
    assert_ne!(
        restored.catalog.get(party.id).unwrap(),
        &changed.change.current
    );
    assert_eq!(restored.latest_recorded_at, original.latest_recorded_at);
    assert_eq!(restored.count, original.count);
    assert_eq!(restored.account, original.account);
    assert_eq!(restored.report, original.report);
    assert_eq!(restored.graph, original.graph);
    assert_eq!(restored.source, original.source);
    assert_eq!(restored.activity, original.activity);
    assert_eq!(restored.treatment, original.treatment);
    assert_eq!(restored.known, original.known);
    assert_eq!(restored.prior_report_review, None);
    let mut current_query = query;
    current_query.facts.expected_stock_revision = db
        .hub_group(owner(), original.count.before.id)
        .await
        .unwrap()
        .revision;
    current_query.facts.expected_catalog_revision = changed.change.catalog_revision;
    let review = db
        .hub_count_explanation_review(owner(), operation(5), current_query, at(190))
        .await
        .unwrap();
    assert_eq!(
        review.outcome,
        crate::HubCountExplanationReviewOutcome::Blocked(
            harvestcircle_domain::hub::corrections::QuantityEffectError::UncertainSubmission
        )
    );
    assert_eq!(
        db.hub_submission(owner(), operation(40))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn historical_count_facts_do_not_substitute_later_materialized_positions() {
    let (fixture, db, query) =
        crate::hub::corrections::count_review::tests::world_with_later_split(true).await;
    let original = facts(&db, query.clone()).await.unwrap();
    let mut raw = fixture.raw().await;
    sqlx::query("UPDATE hub_stock_positions SET packs=CASE WHEN group_id=? THEN 15 ELSE 3 END")
        .bind(original.count.before.id.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    let restored = facts(&db, query.clone()).await.unwrap();
    assert_eq!(restored.count, original.count);
    assert_eq!(restored.graph, original.graph);
    assert_eq!(restored.source, original.source);
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(5), query.clone(), at(150))
            .await
            .err()
            .unwrap()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    let mut missing = query.clone();
    missing.facts.report = HubRecordId::from_bytes([99; 16]).unwrap();
    assert_eq!(
        db.hub_count_explanation_review(owner(), operation(5), missing, at(150))
            .await
            .err()
            .unwrap()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn historical_count_facts_reject_corrupt_journal_envelope_without_writes() {
    let (fixture, db, query) = crate::hub::corrections::count_review::tests::world().await;
    facts(&db, query.clone()).await.unwrap();
    let mut raw = fixture.raw().await;
    let triggers: Vec<(String, String)> = sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND tbl_name='hub_stock_state' ORDER BY name")
        .fetch_all(&mut raw).await.unwrap();
    for (name, _) in &triggers {
        // Audited trigger identifiers from this fresh, validated owned fixture.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP TRIGGER \"{}\"",
            name.replace('"', "\"\"")
        )))
        .execute(&mut raw)
        .await
        .unwrap();
    }
    sqlx::query("UPDATE hub_stock_state SET event_count=event_count+1")
        .execute(&mut raw)
        .await
        .unwrap();
    for (_, trigger) in triggers {
        // Restore the exact original fixture trigger, including every guard.
        sqlx::query(sqlx::AssertSqlSafe(trigger))
            .execute(&mut raw)
            .await
            .unwrap();
    }
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        facts(&db, query).await.err().unwrap().code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn report_review_markers_follow_immutable_owners_at_the_cut_and_reject_cleared_projection() {
    use harvestcircle_domain::hub::actuals::{ActualNewDepartureCommand, ActualReviewFacts};
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
    let mut receiving = command(&catalog);
    receiving.occurred = Occurrence::Known(at(100));
    let crate::HubMutationResult::Received(receipt) = db
        .submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(receiving)),
            at(100),
        )
        .await
        .unwrap()
    else {
        panic!("receipt")
    };
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(crate::hub::stock::tests::id(2)),
                recipient: Some(crate::hub::stock::tests::id(3)),
                packs: Some(WholePacks::from_value(1).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Known(at(110)),
                observation: note(),
            })),
            at(120),
        )
        .await
        .unwrap()
    else {
        panic!("report")
    };
    let current = db
        .hub_group(owner(), receipt.original.position.group)
        .await
        .unwrap();
    let cut = current.revision;
    assert_eq!(marker(&db, report.original.id, cut).await.unwrap(), None);
    db.submit_hub_mutation(
        scope,
        operation(3),
        HubMutation::ActualReview(Box::new(ActualNewDepartureCommand {
            facts: ActualReviewFacts {
                report: report.original.id,
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                group: current.group.id,
                expected_group_revision: current.group.revision,
                packs: WholePacks::from_value(1).unwrap(),
                recipient: crate::hub::stock::tests::id(3),
                occurred: Occurrence::Known(at(110)),
                source_evidence: note(),
                quantity_evidence: note(),
                recipient_evidence: note(),
                timing_evidence: note(),
            },
            source_operation: crate::hub::actuals::common::operation_id(&operation(1)).unwrap(),
            source_group: current.group.id,
            duplicate_review_basis: note(),
        })),
        at(130),
    )
    .await
    .unwrap();
    let after = db
        .hub_group(owner(), current.group.id)
        .await
        .unwrap()
        .revision;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(marker(&db, report.original.id, cut).await.unwrap(), None);
    assert_eq!(
        marker(&db, report.original.id, after).await.unwrap(),
        Some(crate::hub::actuals::common::operation_id(&operation(3)).unwrap())
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    let mut raw = fixture.raw().await;
    let guard: String = sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE type='trigger' AND name='hub_actual_report_review_update_guard'")
        .fetch_one(&mut raw).await.unwrap();
    sqlx::query("DROP TRIGGER hub_actual_report_review_update_guard")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_actual_report_reviews SET review_operation_id=NULL WHERE report_id=?")
        .bind(report.original.id.as_bytes().as_slice())
        .execute(&mut raw)
        .await
        .unwrap();
    // Audited exact CREATE TRIGGER from this freshly validated owned fixture.
    sqlx::query(sqlx::AssertSqlSafe(guard.as_str()))
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let corrupt = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(marker(&db, report.original.id, cut).await.unwrap(), None);
    assert_eq!(
        marker(&db, report.original.id, after)
            .await
            .err()
            .unwrap()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        corrupt
    );
    db.close().await.unwrap();
}
