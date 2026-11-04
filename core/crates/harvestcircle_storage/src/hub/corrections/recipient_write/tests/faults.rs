use super::*;
use harvestcircle_domain::SafeErrorCode;
use sqlx::{AssertSqlSafe, Connection};

#[tokio::test]
async fn recipient_root_admission_prior_and_scope_faults_reject_original_and_current_without_repair()
 {
    for (table, sql) in [
        (
            "hub_corrections",
            "UPDATE hub_corrections SET result_hash=zeroblob(32) WHERE operation_id=?",
        ),
        (
            "hub_corrections",
            "DELETE FROM hub_corrections WHERE operation_id=?",
        ),
        (
            "hub_corrections",
            "UPDATE hub_corrections SET previous_operation_id=operation_id WHERE operation_id=?",
        ),
        (
            "hub_corrections",
            "UPDATE hub_corrections SET target_operation_id='01900000-0000-7000-8000-000000000001' WHERE operation_id=?",
        ),
        (
            "hub_corrections",
            "UPDATE hub_corrections SET subject_id=x'58585858585858585858585858585858' WHERE operation_id=?",
        ),
        (
            "hub_correction_scope_links",
            "DELETE FROM hub_correction_scope_links WHERE correction_operation_id=?",
        ),
        (
            "hub_correction_scope_links",
            "INSERT INTO hub_correction_scope_links (correction_operation_id,target_kind,target_id,introduced_operation_id) SELECT operation_id,'population',x'58585858585858585858585858585858',operation_id FROM hub_corrections WHERE operation_id=?",
        ),
        (
            "hub_submissions",
            "DELETE FROM hub_submissions WHERE operation_id=?",
        ),
        (
            "hub_submissions",
            "UPDATE hub_submissions SET capture_hash=zeroblob(32) WHERE operation_id=?",
        ),
    ] {
        let (fixture, db, scope, input) = world().await;
        db.submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::RecipientAttribution(Box::new(input)),
            at(200),
        )
        .await
        .unwrap();
        let mut raw = fixture.raw().await;
        sqlx::query("PRAGMA foreign_keys=OFF")
            .execute(&mut raw)
            .await
            .unwrap();
        let triggers: Vec<(String, String)> = sqlx::query_as(
            "SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name=? ORDER BY name",
        )
        .bind(table)
        .fetch_all(&mut raw)
        .await
        .unwrap();
        for (name, _) in &triggers {
            sqlx::query(AssertSqlSafe(format!(
                "DROP TRIGGER \"{}\"",
                name.replace('"', "\"\"")
            )))
            .execute(&mut raw)
            .await
            .unwrap();
        }
        assert!(
            sqlx::query(sql)
                .bind(operation(1900).as_str())
                .execute(&mut raw)
                .await
                .unwrap()
                .rows_affected()
                > 0,
            "{table} {sql}"
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
            db.hub_recipient_attribution_original(owner(), operation(1900))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "{table} {sql}"
        );
        assert_eq!(
            db.hub_trace_page(owner(), crate::HubTraceQuery::default())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "{table} {sql}"
        );
        assert_eq!(
            crate::hub::custody::tests::material(&fixture).await,
            damaged
        );
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn resealed_admission_material_and_result_cannot_replace_the_canonical_recipient_cut() {
    let (fixture, db, scope, input) = world().await;
    let crate::HubMutationResult::RecipientAttributed(posted) = db
        .submit_hub_mutation(
            scope,
            operation(1900),
            HubMutation::RecipientAttribution(Box::new(input)),
            at(200),
        )
        .await
        .unwrap()
    else {
        panic!("recipient")
    };
    let mut raw = fixture.raw().await;
    let original: Vec<u8> =
        sqlx::query_scalar("SELECT capture_body FROM hub_submissions WHERE operation_id=?")
            .bind(operation(1900).as_str())
            .fetch_one(&mut raw)
            .await
            .unwrap();
    raw.close().await.unwrap();
    let text = String::from_utf8(original).unwrap();
    let target = "\"latest_recorded_at\":100";
    assert_eq!(text.matches(target).count(), 1);
    let changed = text
        .replace(target, "\"latest_recorded_at\":0")
        .into_bytes();
    reseal_capture(&fixture, &db, posted.original, changed).await;
    let damaged = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_recipient_attribution_original(owner(), operation(1900))
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

// This owned fixture's recipient operation is1900. Recomputing public hashes is
// deliberate fault injection, never a native grant or a replacement original.
pub(super) async fn reseal_capture(
    fixture: &Fixture,
    db: &Database,
    plan: harvestcircle_domain::hub::corrections::RecipientAttributionCorrection,
    changed: Vec<u8>,
) {
    // Metadata inspection verifies syntax and computes the changed public hash;
    // it does not validate these facts or make them native recipient evidence.
    let metadata =
        harvestcircle_application::hub_submission::inspect_hub_submission(&changed).unwrap();
    let digest = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                let hub = reference::authorize_on(tx, owner()).await?;
                let op = stock::read::operation_on(tx, hub.id(), &operation(1900))
                    .await?
                    .ok_or_else(corrupt_storage)?;
                super::super::super::recipient_ledger::digest(&op, &plan, metadata.capture_digest)
            })
        })
        .await
        .unwrap();
    let mut raw = fixture.raw().await;
    let triggers:Vec<(String,String)>=sqlx::query_as("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('hub_submissions','hub_corrections') ORDER BY name").fetch_all(&mut raw).await.unwrap();
    for (name, _) in &triggers {
        sqlx::query(AssertSqlSafe(format!(
            "DROP TRIGGER \"{}\"",
            name.replace('"', "\"\"")
        )))
        .execute(&mut raw)
        .await
        .unwrap();
    }
    sqlx::query("UPDATE hub_submissions SET capture_body=?,capture_hash=? WHERE operation_id=?")
        .bind(changed)
        .bind(metadata.capture_digest.as_slice())
        .bind(operation(1900).as_str())
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("UPDATE hub_corrections SET result_hash=? WHERE operation_id=?")
        .bind(digest.as_slice())
        .bind(operation(1900).as_str())
        .execute(&mut raw)
        .await
        .unwrap();
    for (_, ddl) in triggers {
        sqlx::query(AssertSqlSafe(ddl))
            .execute(&mut raw)
            .await
            .unwrap();
    }
    raw.close().await.unwrap();
}

#[tokio::test]
async fn fully_resealed_capture_cannot_omit_another_population_or_original_paper() {
    for field in ["populations", "issued_versions"] {
        let (fixture, db, scope, input) = world().await;
        let crate::HubMutationResult::RecipientAttributed(posted) = db
            .submit_hub_mutation(
                scope,
                operation(1900),
                HubMutation::RecipientAttribution(Box::new(input)),
                at(200),
            )
            .await
            .unwrap()
        else {
            panic!("recipient")
        };
        assert_eq!(posted.original.original.populations.len(), 2);
        assert_eq!(posted.original.original.issued_versions.len(), 1);
        let mut raw = fixture.raw().await;
        let original: Vec<u8> =
            sqlx::query_scalar("SELECT capture_body FROM hub_submissions WHERE operation_id=?")
                .bind(operation(1900).as_str())
                .fetch_one(&mut raw)
                .await
                .unwrap();
        raw.close().await.unwrap();
        let text = String::from_utf8(original).unwrap();
        let whole = if field == "populations" {
            &posted.original.original.populations
        } else {
            &posted.original.original.issued_versions
        };
        let target = format!("\"{field}\":{}", serde_json::to_string(whole).unwrap());
        let replacement = format!(
            "\"{field}\":{}",
            serde_json::to_string(&whole[..whole.len() - 1]).unwrap()
        );
        assert_eq!(text.matches(&target).count(), 1, "{field}");
        let changed = text.replace(&target, &replacement).into_bytes();
        reseal_capture(&fixture, &db, posted.original, changed).await;
        let damaged = crate::hub::custody::tests::material(&fixture).await;
        assert_eq!(
            db.hub_recipient_attribution_original(owner(), operation(1900))
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "{field}"
        );
        assert_eq!(
            db.hub_trace_page(owner(), crate::HubTraceQuery::default())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::StorageCorrupt,
            "{field}"
        );
        assert_eq!(
            crate::hub::custody::tests::material(&fixture).await,
            damaged
        );
        db.close().await.unwrap();
    }
}

#[tokio::test]
async fn damaged_recipient_root_cannot_hide_behind_an_unrelated_receipt_selection() {
    let (fixture, db, scope, input) = world().await;
    db.submit_hub_mutation(
        scope,
        operation(1900),
        HubMutation::RecipientAttribution(Box::new(input)),
        at(200),
    )
    .await
    .unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    let mut input = crate::hub::stock::tests::command(&catalog);
    input.expected_stock_revision = db.hub_stock_page(owner(), query()).await.unwrap().revision;
    let crate::HubMutationResult::Received(unrelated) = db
        .submit_hub_mutation(
            scope,
            operation(2000),
            HubMutation::Receive(Box::new(input)),
            at(210),
        )
        .await
        .unwrap()
    else {
        panic!("receipt")
    };
    let mut query = crate::HubTraceQuery::default();
    query.selection.scope =
        harvestcircle_domain::hub::trace::TraceScope::Receipt(unrelated.original.trace.id);
    let page = db.hub_trace_page(owner(), query.clone()).await.unwrap();
    assert_eq!(page.total, 1);
    assert!(page.rows[0].recipient_corrections.is_empty());
    crate::hub::custody::tests::corrupt(&fixture,"hub_corrections_no_update","UPDATE hub_corrections SET result_hash=zeroblob(32) WHERE operation_id='01900000-0000-7000-8000-00000000076c'").await;
    let damaged = crate::hub::custody::tests::material(&fixture).await;
    assert_eq!(
        db.hub_trace_page(owner(), query).await.unwrap_err().code(),
        SafeErrorCode::StorageCorrupt
    );
    assert_eq!(
        crate::hub::custody::tests::material(&fixture).await,
        damaged
    );
    db.close().await.unwrap();
}
