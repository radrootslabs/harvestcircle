//! Current-mode composition uses a new disposable profile, never the retained19 worlds.
use super::*;

async fn link(
    db: &Database,
    scope: HubSubmissionScope,
    trace: HubRecordId,
    n: u32,
    recipient: Option<HubRecordId>,
    recorded: i64,
) -> crate::HubActualReviewReceipt {
    let crate::HubMutationResult::ActualReported(report) = db
        .submit_hub_mutation(
            scope,
            operation(n),
            HubMutation::ActualReport(Box::new(ActualReportCommand {
                variant: Some(id(2)),
                recipient,
                packs: Some(WholePacks::from_value(2).unwrap()),
                sources: ActualReportSources::Unknown,
                occurred: Occurrence::Unknown,
                observation: note(),
            })),
            at(recorded),
        )
        .await
        .unwrap()
    else {
        panic!("raw qualified link report")
    };
    let command = ActualDuplicateLink {
        report: report.original.id,
        known_operation: crate::hub::actuals::common::operation_id(&operation(81)).unwrap(),
        known_trace: trace,
        known_variant: report.original.report.variant.unwrap(),
        verification: note(),
    };
    let before = db
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
    let preview = db
        .hub_actual_link_preview(owner(), operation(n + 1), command.clone(), at(recorded + 1))
        .await
        .unwrap();
    let crate::HubMutationResult::ActualReviewed(link) = db
        .submit_hub_mutation(
            scope,
            operation(n + 1),
            HubMutation::ActualLink(Box::new(command)),
            at(recorded + 1),
        )
        .await
        .unwrap()
    else {
        panic!("qualified zero-effect link")
    };
    let crate::HubActualReviewOriginal::Duplicate(plan) = &link.original else {
        panic!("duplicate original")
    };
    assert_eq!(**plan, preview.plan);
    let after = db
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
    assert_eq!(after.actual_departures, before.actual_departures);
    assert_eq!(after.on_hand, before.on_hand);
    assert_eq!(after.quantity_corrections, before.quantity_corrections);
    *link
}

#[tokio::test]
async fn native20_zero_quantity_prior_unknown_current_recipients_and_source_changes_keep_link_cut_originals()
 {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 20);
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let (_, cuts, scope) = Box::pin(drive_native_observed(&db, true)).await;
    let trace = cuts.last().unwrap().records[0].original.trace;
    let target = crate::hub::actuals::common::operation_id(&operation(81)).unwrap();
    let unknown = command(
        &db,
        target,
        None,
        Some(crate::hub::actuals::common::operation_id(&operation(KNOWN_RECIPIENT)).unwrap()),
    )
    .await;
    db.submit_hub_mutation(
        scope,
        operation(30100),
        HubMutation::RecipientAttribution(Box::new(unknown)),
        at(400),
    )
    .await
    .unwrap();
    let first = link(&db, scope, trace, 30101, None, 410).await;
    let crate::HubActualReviewOriginal::Duplicate(plan) = &first.original else {
        panic!("first link")
    };
    let meaning = plan.interpretation.as_ref().unwrap();
    assert_eq!(meaning.view().unwrap().effective_packs.value(), 0);
    assert_eq!(meaning.view().unwrap().original_packs.value(), 2);
    assert!(matches!(
        harvestcircle_domain::hub::corrections::current_recipient_attribution(
            &meaning.recipient_original,
            &meaning.recipient_history
        )
        .unwrap(),
        AmendedRecipient::Unknown
    ));
    let known = command(
        &db,
        target,
        Some(id(4)),
        Some(crate::hub::actuals::common::operation_id(&operation(30100)).unwrap()),
    )
    .await;
    db.submit_hub_mutation(
        scope,
        operation(30103),
        HubMutation::RecipientAttribution(Box::new(known)),
        at(430),
    )
    .await
    .unwrap();
    let second = link(&db, scope, trace, 30104, Some(id(4)), 440).await;
    let crate::HubActualReviewOriginal::Duplicate(plan) = &second.original else {
        panic!("second link")
    };
    let meaning = plan.interpretation.as_ref().unwrap();
    assert_eq!(meaning.view().unwrap().effective_packs.value(), 0);
    assert!(
        matches!(harvestcircle_domain::hub::corrections::current_recipient_attribution(&meaning.recipient_original,&meaning.recipient_history).unwrap(),AmendedRecipient::Known(party) if party.id==id(4))
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), first.operation.clone())
            .await
            .unwrap(),
        Some(first.clone())
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    for original in [&first, &second] {
        assert_eq!(
            db.hub_actual_review_operation(owner(), original.operation.clone())
                .await
                .unwrap(),
            Some((*original).clone())
        );
    }
    assert!(matches!(
        db.hub_submission(owner(), operation(UNRESOLVED))
            .await
            .unwrap()
            .unwrap()
            .status,
        crate::HubSubmissionStatus::Recorded
    ));
    assert!(
        db.hub_actual_report_operation(owner(), operation(UNRESOLVED))
            .await
            .unwrap()
            .is_some()
    );
    // A later damaged recipient original cannot poison the earlier link cut;
    // the link which included that amendment must fail closed without repair.
    use sqlx::{AssertSqlSafe, Connection};
    let mut raw = fixture.raw().await;
    let unresolved:i64=sqlx::query_scalar("SELECT count(*) FROM hub_actual_report_reviews m JOIN hub_actual_reports r ON r.report_id=m.report_id WHERE r.operation_id=? AND m.review_operation_id IS NULL").bind(operation(UNRESOLVED).as_str()).fetch_one(&mut raw).await.unwrap();
    assert_eq!(unresolved, 1);
    let ddl: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name='hub_corrections_no_update'")
            .fetch_one(&mut raw)
            .await
            .unwrap();
    sqlx::query("DROP TRIGGER hub_corrections_no_update")
        .execute(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query("UPDATE hub_corrections SET result_hash=zeroblob(32) WHERE operation_id=?")
            .bind(operation(30103).as_str())
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    sqlx::query(AssertSqlSafe(ddl))
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_actual_review_operation(owner(), first.operation.clone())
            .await
            .unwrap(),
        Some(first)
    );
    assert_eq!(
        db.hub_actual_review_operation(owner(), second.operation)
            .await
            .unwrap_err()
            .code(),
        harvestcircle_domain::SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
