use super::*;
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        corrections::CorrectionImpact,
        departure::DepartureCommand,
        dispatch::{DispatchAction, DispatchCommand, DispatchLineInput, DispatchSelection},
        stock::Occurrence,
    },
};
use sqlx::{AssertSqlSafe, Connection};
mod historical_prefix;

async fn issued_world() -> (
    Fixture,
    Database,
    HubSubmissionScope,
    SourceAttributionCommand,
    crate::HubDispatchReceipt,
) {
    let fixture = Fixture::new();
    let db = fixture.open().await;
    let paper = crate::hub::handling::departure::tests::issued_departure_fixture(&db).await;
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
    let input = command(
        &db,
        paper.record.versions[0].material.lines[0].trace.id,
        source(),
        None,
    )
    .await;
    (fixture, db, scope, input, paper)
}

#[tokio::test]
async fn issued_without_departure_is_complete_source_scope_without_a_fabricated_departure() {
    let (_fixture, db, scope, input, paper) = issued_world().await;
    let version = paper.record.versions[0].id;
    let recipient = paper.record.versions[0].material.recipient.id;
    let preview = db
        .hub_source_attribution_review(owner(), operation(1900), input.clone(), at(200))
        .await
        .unwrap();
    let crate::HubAttributionReviewOutcome::Candidate(plan) = preview.outcome else {
        panic!("source")
    };
    assert!(
        plan.impact
            .contains(CorrectionImpact::IssuedVersion(version))
    );
    assert!(plan.impact.contains(CorrectionImpact::Recipient(recipient)));
    assert!(
        !plan
            .impact
            .rows()
            .iter()
            .any(|r| matches!(r, CorrectionImpact::Departure(_)))
    );
    let crate::HubMutationResult::SourceAttributed(posted) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::SourceAttribution(Box::new(input)),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    assert_eq!(posted.original, *plan);
    assert_eq!(
        db.hub_dispatch_operation(owner(), operation(21))
            .await
            .unwrap(),
        Some(paper)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn amended_source_blocks_current_material_and_departure_but_preserves_exact_old_issue_and_closure()
 {
    let (fixture, db, scope, input, paper) = issued_world().await;
    let version = &paper.record.versions[0];
    let version_id = version.id;
    let html =
        harvestcircle_application::hub_output::identification_output(&paper.record, version_id)
            .unwrap();
    let crate::HubMutationResult::SourceAttributed(source_posted) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::SourceAttribution(Box::new(input)),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    let review = db
        .hub_departure_review(
            owner(),
            crate::HubDepartureReviewInput {
                preparation: paper.record.id,
                expected_preparation_revision: paper.record.revision,
                identification: version_id,
            },
            at(210),
        )
        .await
        .unwrap();
    assert_eq!(
        review.block.as_ref().map(|error| error.code()),
        Some(SafeErrorCode::HubConflict)
    );
    let before = crate::hub::custody::tests::material(&fixture).await;
    let attempted = db
        .submit_hub_mutation(
            scope,
            operation(1910),
            HubMutation::Departure(Box::new(DepartureCommand {
                expected_stock_revision: review.view.revision,
                expected_catalog_revision: review.view.catalog_revision,
                preparation: paper.record.id,
                expected_preparation_revision: paper.record.revision,
                identification: version_id,
                occurred: Occurrence::Unknown,
                observation: SourceProvenance::new(
                    "Old source material cannot grant current departure",
                )
                .unwrap(),
            })),
            at(210),
        )
        .await
        .unwrap_err();
    assert_eq!(attempted.code(), SafeErrorCode::HubConflict);
    let physical = |rows: Vec<(String, Vec<String>)>| {
        rows.into_iter()
            .filter(|(name, _)| {
                !matches!(
                    name.as_str(),
                    "hub_submissions" | "hub_submission_rejections"
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        physical(crate::hub::custody::tests::material(&fixture).await),
        physical(before)
    );
    let current = db.hub_stock_page(owner(), query()).await.unwrap();
    let selection = DispatchSelection::new(
        version
            .material
            .lines
            .iter()
            .map(|l| DispatchLineInput {
                group: l.group,
                expected_group_revision: l.group_revision,
                packs: l.packs,
            })
            .collect(),
    )
    .unwrap();
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(1911),
            HubMutation::Dispatch(Box::new(DispatchCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                action: DispatchAction::Prepare {
                    recipient: version.material.recipient.id,
                    lines: selection
                },
                observation: SourceProvenance::new(
                    "Current source material needs qualified linked review"
                )
                .unwrap()
            })),
            at(211)
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubConflict
    );
    assert_eq!(
        physical(crate::hub::custody::tests::material(&fixture).await),
        physical(before)
    );
    db.submit_hub_mutation(
        scope,
        operation(1912),
        HubMutation::Dispatch(Box::new(DispatchCommand {
            expected_stock_revision: current.revision,
            expected_catalog_revision: current.catalog_revision,
            action: DispatchAction::MarkUnused {
                preparation: paper.record.id,
                expected_revision: paper.record.revision,
            },
            observation: SourceProvenance::new(
                "Explicit unused closure retains exact old source paperwork",
            )
            .unwrap(),
        })),
        at(220),
    )
    .await
    .unwrap();
    let original = db
        .hub_dispatch_operation(owner(), operation(21))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original, paper);
    assert_eq!(
        harvestcircle_application::hub_output::identification_output(&original.record, version_id)
            .unwrap(),
        html
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*source_posted.clone())
    );
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(
        db.hub_dispatch_operation(owner(), operation(21))
            .await
            .unwrap(),
        Some(paper)
    );
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1900))
            .await
            .unwrap(),
        Some(*source_posted)
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn omitted_issued_scope_cannot_be_resealed_into_an_existing_source_original() {
    let (fixture, db, scope, input, _paper) = issued_world().await;
    let preview = db
        .hub_source_attribution_review(owner(), operation(1900), input.clone(), at(200))
        .await
        .unwrap();
    assert_eq!(preview.issued_versions.len(), 1);
    let crate::HubMutationResult::SourceAttributed(posted) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::SourceAttribution(Box::new(input)),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("source")
    };
    let mut raw = fixture.raw().await;
    let body: Vec<u8> =
        sqlx::query_scalar("SELECT capture_body FROM hub_submissions WHERE operation_id=?")
            .bind(operation(1900).as_str())
            .fetch_one(&mut raw)
            .await
            .unwrap();
    raw.close().await.unwrap();
    let text = String::from_utf8(body).unwrap();
    let target = format!(
        "\"issued_versions\":{}",
        serde_json::to_string(&preview.issued_versions).unwrap()
    );
    assert_eq!(text.matches(&target).count(), 1);
    let changed = text.replace(&target, "\"issued_versions\":[]").into_bytes();
    super::faults::reseal_capture(&fixture, &db, posted.original, changed).await;
    let damaged = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1900))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_trace_page(owner(), crate::HubTraceQuery::default())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        damaged
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn unrelated_issued_original_cannot_disappear_behind_source_trace_selection() {
    let (fixture, db, scope, input, paper) = issued_world().await;
    let catalog = db.hub_references(owner()).await.unwrap();
    let mut receipt = receiving(&catalog);
    receipt.expected_stock_revision = db.hub_stock_page(owner(), query()).await.unwrap().revision;
    let crate::HubMutationResult::Received(received) = db
        .submit_hub_mutation(
            scope,
            operation(2),
            HubMutation::Receive(Box::new(receipt)),
            at(160),
        )
        .await
        .unwrap()
    else {
        panic!("receipt")
    };
    let current = db.hub_stock_page(owner(), query()).await.unwrap();
    let crate::HubMutationResult::Dispatch(prepared) = db
        .submit_hub_mutation(
            scope,
            operation(30),
            HubMutation::Dispatch(Box::new(DispatchCommand {
                expected_stock_revision: current.revision,
                expected_catalog_revision: current.catalog_revision,
                action: DispatchAction::Prepare {
                    recipient: id(9),
                    lines: DispatchSelection::new(vec![DispatchLineInput {
                        group: received.original.position.group,
                        expected_group_revision: received.original.position.revision,
                        packs: received.original.position.packs,
                    }])
                    .unwrap(),
                },
                observation: SourceProvenance::new(
                    "Unrelated source population has its own fictional issued copy",
                )
                .unwrap(),
            })),
            at(170),
        )
        .await
        .unwrap()
    else {
        panic!("prepare")
    };
    let crate::HubMutationResult::Dispatch(issued) = db
        .submit_hub_mutation(
            scope,
            operation(31),
            HubMutation::Dispatch(Box::new(DispatchCommand {
                expected_stock_revision: prepared.record.updated.stock_revision,
                expected_catalog_revision: prepared.catalog_revision,
                action: DispatchAction::Issue {
                    preparation: prepared.record.id,
                    expected_revision: prepared.record.revision,
                },
                observation: SourceProvenance::new(
                    "Independent issue, never the target source population",
                )
                .unwrap(),
            })),
            at(180),
        )
        .await
        .unwrap()
    else {
        panic!("issue")
    };
    let input = command(&db, input.trace, source(), None).await;
    let preview = db
        .hub_source_attribution_review(owner(), operation(1900), input.clone(), at(200))
        .await
        .unwrap();
    assert_eq!(preview.issued_versions.len(), 1);
    assert_eq!(
        preview.issued_versions[0].version,
        paper.record.versions[0].id
    );
    assert_ne!(
        preview.issued_versions[0].version,
        issued.record.versions[0].id
    );
    db.submit_hub_mutation(
        scope,
        operation(1900),
        HubMutation::SourceAttribution(Box::new(input)),
        at(200),
    )
    .await
    .unwrap();
    let mut raw = fixture.raw().await;
    sqlx::query("PRAGMA foreign_keys=OFF")
        .execute(&mut raw)
        .await
        .unwrap();
    let triggers:Vec<(String,String)>=sqlx::query_as("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name='hub_dispatch_changes' ORDER BY name").fetch_all(&mut raw).await.unwrap();
    for (name, _) in &triggers {
        sqlx::query(AssertSqlSafe(format!(
            "DROP TRIGGER \"{}\"",
            name.replace('"', "\"\"")
        )))
        .execute(&mut raw)
        .await
        .unwrap();
    }
    assert_eq!(
        sqlx::query("DELETE FROM hub_dispatch_changes WHERE operation_id=?")
            .bind(operation(31).as_str())
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    for (_, ddl) in triggers {
        sqlx::query(AssertSqlSafe(ddl))
            .execute(&mut raw)
            .await
            .unwrap();
    }
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let damaged = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_source_attribution_original(owner(), operation(1900))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        db.hub_trace_page(owner(), crate::HubTraceQuery::default())
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        damaged
    );
    db.close().await.unwrap();
}
