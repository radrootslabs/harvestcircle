use super::*;
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        counts::{CountObserveCommand, CountPostCommand},
        movement::PhysicalIdentification,
        reference::{HubLabel, WholePacks},
        stock::Occurrence,
    },
};
use sqlx::{AssertSqlSafe, Connection};

pub(super) async fn snapshot(
    db: &Database,
    cut: Option<HubRevision>,
) -> Result<serde_json::Value, SafeError> {
    db.host().transaction(|tx| Box::pin(async move {
        let hub = reference::authorize_on(tx, owner()).await?;
        let mut reader = OriginalReader::new(tx);
        let ledger = Box::pin(super::super::super::source_ledger::load_through_on(&mut reader, hub.id(), cut)).await?;
        let populations: Vec<_> = ledger.populations.into_iter().map(|p| serde_json::json!({
            "operation":p.operation.as_str(), "revision":p.revision, "catalog":p.catalog_revision,
            "recorded":p.recorded_at, "actor":p.actor, "population":p.population,
        })).collect();
        Ok(serde_json::json!({"populations":populations, "records":ledger.records, "issued":ledger.issued_versions}))
    })).await.map_err(map_transaction_error)
}

pub(super) async fn damage(
    fixture: &Fixture,
    table: &str,
    update: &str,
    identity: &str,
    foreign_keys: bool,
) {
    let mut raw = fixture.raw().await;
    let triggers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND tbl_name=? ORDER BY name",
    )
    .bind(table)
    .fetch_all(&mut raw)
    .await
    .unwrap();
    assert!(!triggers.is_empty());
    if !foreign_keys {
        sqlx::query("PRAGMA foreign_keys=OFF")
            .execute(&mut raw)
            .await
            .unwrap();
    }
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
        sqlx::query(AssertSqlSafe(update.to_owned()))
            .bind(identity)
            .execute(&mut raw)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    for (_, trigger) in triggers {
        sqlx::query(AssertSqlSafe(trigger))
            .execute(&mut raw)
            .await
            .unwrap();
    }
    if !foreign_keys {
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&mut raw)
            .await
            .unwrap();
    }
    raw.close().await.unwrap();
}

async fn rich_world() -> (
    Fixture,
    Database,
    HubRevision,
    serde_json::Value,
    HubRevision,
) {
    let (fixture, db, scope, input) = world().await;
    let trace = input.trace;
    let crate::HubMutationResult::SourceAttributed(first) = db
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
    let cut = first.original.stamp.stock_revision;
    let saved = snapshot(&db, Some(cut)).await.unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let mut incoming = receiving(&catalog);
    incoming.expected_stock_revision = page.revision;
    incoming.source = source();
    let crate::HubMutationResult::Received(second) = db
        .submit_hub_mutation(
            scope,
            operation(1902),
            HubMutation::Receive(Box::new(incoming)),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("reuse")
    };
    let group = db
        .hub_group(owner(), second.original.position.group)
        .await
        .unwrap();
    let crate::HubMutationResult::CountObserved(observed) = db
        .submit_hub_mutation(
            scope,
            operation(1903),
            HubMutation::CountObservation(Box::new(CountObserveCommand {
                expected_stock_revision: group.revision,
                expected_catalog_revision: group.catalog_revision,
                group: group.group.id,
                expected_group_revision: group.group.revision,
                counted_packs: WholePacks::from_value(22).unwrap(),
                cutoff: Occurrence::Known(at(215)),
                observation: SourceProvenance::new(
                    "Later independently counted fictional extra two",
                )
                .unwrap(),
            })),
            at(220),
        )
        .await
        .unwrap()
    else {
        panic!("observe")
    };
    let view = db
        .hub_count_review(owner(), observed.original.id)
        .await
        .unwrap();
    let result = db
        .submit_hub_mutation(
            scope,
            operation(1904),
            HubMutation::CountPosting(Box::new(CountPostCommand {
                expected_stock_revision: view.stock_revision,
                expected_catalog_revision: view.catalog_revision,
                count: observed.original.id,
                reviewed_activity: true,
                surplus_identification: Some(PhysicalIdentification {
                    label: HubLabel::new("Later distinct counted extra two").unwrap(),
                    observation: SourceProvenance::new(
                        "Separate restricted surplus, not an old source origin",
                    )
                    .unwrap(),
                }),
                reason: SourceProvenance::new("Current count and distinct surplus reviewed")
                    .unwrap(),
            })),
            at(230),
        )
        .await
        .unwrap();
    assert!(matches!(result, crate::HubMutationResult::CountPosted(_)));
    let next = command(
        &db,
        trace,
        SourceAttribution::Unknown,
        Some(first.original.stamp.operation),
    )
    .await;
    let crate::HubMutationResult::SourceAttributed(last) = db
        .submit_hub_mutation(
            scope,
            operation(1905),
            HubMutation::SourceAttribution(Box::new(next)),
            at(240),
        )
        .await
        .unwrap()
    else {
        panic!("source2")
    };
    (fixture, db, cut, saved, last.original.stamp.stock_revision)
}

#[tokio::test]
async fn historical_source_prefix_keeps_first_creator_reuse_and_later_count_surplus_separate_after_reopen()
 {
    let (fixture, db, cut, saved, last) = rich_world().await;
    assert_eq!(saved["populations"].as_array().unwrap().len(), 1);
    assert_eq!(saved["records"].as_array().unwrap().len(), 1);
    let current = snapshot(&db, None).await.unwrap();
    assert_eq!(current["populations"].as_array().unwrap().len(), 3);
    assert_eq!(current["records"].as_array().unwrap().len(), 2);
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(snapshot(&db, Some(cut)).await.unwrap(), saved);
    assert_eq!(snapshot(&db, Some(last)).await.unwrap(), current);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
    let db = fixture.open().await;
    assert_eq!(snapshot(&db, Some(cut)).await.unwrap(), saved);
    assert_eq!(snapshot(&db, None).await.unwrap(), current);
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}

#[tokio::test]
async fn historical_source_prefix_excludes_future_correction_and_count_original_faults_but_current_rejects()
 {
    for (table, update, op) in [
        (
            "hub_corrections",
            "UPDATE hub_corrections SET result_hash=zeroblob(32) WHERE operation_id=?",
            1905,
        ),
        (
            "hub_count_reconciliations",
            "UPDATE hub_count_reconciliations SET result_hash=zeroblob(32) WHERE operation_id=?",
            1904,
        ),
    ] {
        let (fixture, db, cut, saved, last) = rich_world().await;
        damage(&fixture, table, update, operation(op).as_str(), true).await;
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(snapshot(&db, Some(cut)).await.unwrap(), saved, "{table}");
        assert_eq!(
            snapshot(&db, Some(last)).await.unwrap_err().code(),
            SafeErrorCode::StorageCorrupt,
            "{table}"
        );
        assert_eq!(
            snapshot(&db, None).await.unwrap_err().code(),
            SafeErrorCode::StorageCorrupt,
            "{table}"
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn historical_source_prefix_cannot_hide_an_original_creator_behind_a_future_reuse_or_missing_operation()
 {
    for creator in [operation(1902), operation(9000)] {
        let (fixture, db, cut, _, _) = rich_world().await;
        damage(
            &fixture,
            "hub_source_references",
            "UPDATE hub_source_references SET created_operation_id=?",
            creator.as_str(),
            false,
        )
        .await;
        let before = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            snapshot(&db, Some(cut)).await.unwrap_err().code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(
            snapshot(&db, None).await.unwrap_err().code(),
            SafeErrorCode::StorageCorrupt
        );
        assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn historical_source_prefix_excludes_a_later_missing_body_but_rejects_it_when_included() {
    let (fixture, db, scope, input) = world().await;
    let old = input.expected_stock_revision;
    let saved = snapshot(&db, Some(old)).await.unwrap();
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
    damage(
        &fixture,
        "hub_corrections",
        "DELETE FROM hub_corrections WHERE operation_id=?",
        operation(1900).as_str(),
        false,
    )
    .await;
    let before = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(snapshot(&db, Some(old)).await.unwrap(), saved);
    assert_eq!(
        snapshot(&db, Some(posted.original.stamp.stock_revision))
            .await
            .unwrap_err()
            .code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        snapshot(&db, None).await.unwrap_err().code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(crate::hub::custody::tests::material(&fixture).await, before);
    db.close().await.unwrap();
}
