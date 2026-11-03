//! Ordinary admission exhaustion must not strand an existing report closure.
use super::mutations::{report, review, scope};
use super::*;
use harvestcircle_application::hub_submission::HubMutation;

async fn fill_rejected_ordinary_quota(fixture: &Fixture) {
    // Synthetic rejected fixture rows exercise the exact SQL quota. They are
    // deliberately not claimed to be 65536 native captures or replay inputs.
    let mut raw = fixture.raw().await;
    sqlx::query(concat!(
        "WITH RECURSIVE seq(n) AS (VALUES(100000) UNION ALL SELECT n+1 FROM seq WHERE n<165535) ",
        "INSERT INTO hub_submissions (operation_id,hub_id,actor_public_key,kind,runtime_id,generation,input_hash,domain_intent_hash,capture_schema,capture_body,capture_hash,submitted_at_unix_s) ",
        "SELECT printf('01900000-0000-7000-8000-%012x',n),h.hub_id,r.actor_public_key,'receipt',r.runtime_id,r.generation,zeroblob(32),zeroblob(32),1,x'00',zeroblob(32),150 ",
        "FROM seq CROSS JOIN hub_context h CROSS JOIN hub_runtime_session r"
    )).execute(&mut raw).await.unwrap();
    sqlx::query("INSERT INTO hub_submission_rejections (operation_id,code,recorded_at_unix_s) SELECT operation_id,'invalid_input',150 FROM hub_submissions WHERE kind='receipt'")
        .execute(&mut raw).await.unwrap();
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM hub_submissions WHERE kind NOT IN ('hold_release','dispatch_unused','actual_report','actual_review')").fetch_one(&mut raw).await.unwrap(),65536);
    raw.close().await.unwrap();
}

#[tokio::test]
async fn full_ordinary_quota_still_preserves_first_report_and_closes_physical_review() {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let intake = received(&db).await;
    let scope = scope(&db).await;
    db.close().await.unwrap();
    fill_rejected_ordinary_quota(&fixture).await;
    let db = fixture.open().await;
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
        panic!("reserved report admission")
    };
    let command = review(&db, saved.original.id, intake.original.position.group).await;
    db.submit_hub_mutation(
        scope,
        operation(81),
        HubMutation::ActualReview(Box::new(command)),
        at(160),
    )
    .await
    .unwrap();
    assert!(
        db.hub_actual_reports(owner(), HubActualReportQuery::default())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap()
            .group
            .packs,
        packs(18)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn full_ordinary_quota_cannot_block_zero_effect_link_to_existing_actual() {
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
    db.submit_hub_mutation(
        scope,
        operation(81),
        HubMutation::ActualReview(Box::new(command)),
        at(160),
    )
    .await
    .unwrap();
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
        panic!("duplicate")
    };
    db.close().await.unwrap();
    fill_rejected_ordinary_quota(&fixture).await;
    let db = fixture.open().await;
    let before = db
        .hub_group(owner(), intake.original.position.group)
        .await
        .unwrap();
    let link = harvestcircle_domain::hub::actuals::ActualDuplicateLink {
        report: duplicate.original.id,
        known_operation: HubRecordId::from_hex(&operation(81).as_str().replace('-', "")).unwrap(),
        known_trace: intake.original.trace.id,
        known_variant: id(2),
        verification: note("Exact retained physical original; no new stock effect"),
    };
    db.submit_hub_mutation(
        scope,
        operation(83),
        HubMutation::ActualLink(Box::new(link)),
        at(180),
    )
    .await
    .unwrap();
    assert_eq!(
        db.hub_group(owner(), intake.original.position.group)
            .await
            .unwrap()
            .group,
        before.group
    );
    assert!(
        db.hub_actual_reports(owner(), HubActualReportQuery::default())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    db.close().await.unwrap();
}
