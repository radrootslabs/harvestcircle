//! One-use forward consumer of actual retained17. Never replay saved input.
use crate::Database;
use crate::hub::stock::{
    tests::{operation, owner, query},
    upgrade_tests::original_rows,
};
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::{MigrationBuildIdentity, SchemaObject};
use sha2::{Digest, Sha256};
use sqlx::{
    Column, Connection, Row, SqliteConnection, TypeInfo, ValueRef, sqlite::SqliteConnectOptions,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
const ORIGINAL: &str = "e592ac1fa3ea5edb9cf29a55a65078cca4ea32049308d0feebea0be92f9e9c56";
fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}
fn write_new(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
async fn inventory(raw: &mut SqliteConnection) -> serde_json::Value {
    let names = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&mut *raw)
    .await
    .unwrap();
    let mut tables = Vec::new();
    for name in names {
        let query = format!(
            "SELECT * FROM \"{}\" ORDER BY rowid",
            name.replace('"', "\"\"")
        );
        // Schema identifier is quoted with every embedded quote doubled.
        let rows = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
            .fetch_all(&mut *raw)
            .await
            .unwrap();
        let mut hash = Sha256::new();
        hash.update(query.as_bytes());
        for row in &rows {
            hash.update(b"row\0");
            for column in row.columns() {
                hash.update(column.name().as_bytes());
                hash.update([0]);
                let value = row.try_get_raw(column.ordinal()).unwrap();
                let bytes = if value.is_null() {
                    hash.update(b"null\0");
                    Vec::new()
                } else {
                    let kind = value.type_info();
                    hash.update(kind.name().as_bytes());
                    hash.update([0]);
                    match kind.name() {
                        "INTEGER" => row.get::<i64, _>(column.ordinal()).to_be_bytes().to_vec(),
                        "TEXT" => row.get::<String, _>(column.ordinal()).into_bytes(),
                        "BLOB" => row.get::<Vec<u8>, _>(column.ordinal()),
                        kind => panic!("unexpected retained storage type: {kind}"),
                    }
                };
                hash.update(u64::try_from(bytes.len()).unwrap().to_be_bytes());
                hash.update(bytes);
            }
        }
        tables.push(serde_json::json!({"table": name, "rows": rows.len(), "sha256": format!("{:x}", hash.finalize())}));
    }
    serde_json::Value::Array(tables)
}

async fn reader(path: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false)
            .read_only(true)
            .immutable(true),
    )
    .await
    .unwrap()
}
async fn schema17_matches(raw: &mut SqliteConnection) {
    for object in crate::contract::retained_schema17_objects() {
        let row = sqlx::query("SELECT tbl_name,sql FROM sqlite_schema WHERE name=? LIMIT 1")
            .bind(object.name())
            .fetch_one(&mut *raw)
            .await
            .unwrap();
        let digest = SchemaObject::computed_digest(
            object.kind(),
            object.name(),
            &row.get::<String, _>("tbl_name"),
            &row.get::<String, _>("sql"),
        )
        .unwrap();
        assert_eq!(
            digest,
            object.digest(),
            "original17 object {}",
            object.name()
        );
    }
}

#[tokio::test]
#[ignore = "Consumes actual retained17 once; never resets originals, begins a runtime or executes saved pending input"]
async fn retained_actual17_upgrade_preserves_all_originals_runtime_and_pending_claim() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 18);
    assert_eq!(std::env::consts::OS, "macos");
    assert_eq!(std::env::consts::ARCH, "aarch64");
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let retained = project.join("retained_evidence").canonicalize().unwrap();
    let output = retained
        .join("harvestcircle-schema17-before-actuals-v1")
        .canonicalize()
        .unwrap();
    assert_eq!(output.parent(), Some(retained.as_path()));
    let witness_file = output.join("witness.json");
    let witness: serde_json::Value =
        serde_json::from_slice(&fs::read(&witness_file).unwrap()).unwrap();
    assert_eq!(witness["schema"], 17);
    assert_eq!(witness["personal_identity_or_keyring_data"], false);
    assert_eq!(witness["original_sha256"], ORIGINAL);
    assert_eq!(witness["service_schema_objects"], 173);
    assert_eq!(witness["shared_host_schema_objects"], 6);
    let archive = PathBuf::from(witness["original_archive"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let state = PathBuf::from(witness["state_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let profile = PathBuf::from(witness["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(
        archive.starts_with(&output)
            && profile.starts_with(&retained)
            && state.starts_with(&profile)
    );
    assert_eq!(digest(&archive), ORIGINAL);
    assert_eq!(digest(&state), ORIGINAL);
    assert_eq!(
        fs::metadata(state.with_extension("sqlite-wal"))
            .unwrap()
            .len(),
        0
    );
    let mut copied = reader(&archive).await;
    schema17_matches(&mut copied).await;
    let before = inventory(&mut copied).await;
    assert_eq!(before, witness["tables"]);
    assert_eq!(before.as_array().unwrap().len(), 52);
    let original_objects:Vec<(String,String)>=sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(&mut copied).await.unwrap();
    assert_eq!(original_objects.len(), 179);
    let rows17 = original_rows(&mut copied, 17).await;
    let rows16 = original_rows(&mut copied, 16).await;
    assert_eq!(
        rows16
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        witness["original_rows_through16_sha256"].as_str().unwrap()
    );
    copied.close().await.unwrap();
    let mut live = reader(&state).await;
    schema17_matches(&mut live).await;
    assert_eq!(inventory(&mut live).await, before);
    live.close().await.unwrap();
    let context = RuntimeContext::resolve(
        &RadrootsPathResolver::new(
            RadrootsPlatform::current(),
            RadrootsHostEnvironment::default(),
        ),
        RuntimeContextBootstrap::new(
            RadrootsPathProfile::RepoLocal,
            Some(profile),
            RuntimeContextSource::BootstrapCli,
            RuntimeContextSource::SafeDefault,
        )
        .unwrap(),
        ServiceId::new("harvestcircle").unwrap(),
        InstanceId::new("desktop").unwrap(),
    )
    .unwrap();
    let build = MigrationBuildIdentity::new(
        "0.1.0-alpha",
        "089dbda0a3a89ed1d4001a1987577d1cdff0f288",
        "189c49b74b4bafc142b00b76b296477931139e72",
        "1.99.0",
        "aarch64-apple-darwin",
        "retained-actual-reports-native-upgrade-test",
        1,
        18,
        1,
        1,
        1,
    )
    .unwrap();
    let contract = crate::HarvestCircleStorageContract::from_runtime_context(&context).unwrap();
    assert_eq!(contract.paths().state_database(), state.as_path());
    let claim = output.join("actual-schema18-upgrade-1.claim");
    let result = output.join("actual-schema18-upgrade-1-result.json");
    assert!(!claim.exists() && !result.exists());
    // Exact one-use claim precedes the first writable native open. Any subsequent
    // failure requires reconciliation; never remove, replace or rerun this claim.
    write_new(
        &claim,
        b"One-use actual17->18 native consumer; retain any partial state.\n",
    );
    let db = Database::open(&context, 99, 103, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 18);
    let receipt = db
        .hub_receipt_operation(owner(), operation(1))
        .await
        .unwrap()
        .unwrap();
    let opening = db
        .hub_opening_operation(owner(), operation(2))
        .await
        .unwrap()
        .unwrap();
    let count = db
        .hub_count_observation_operation(owner(), operation(3))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.original.trace.original_packs.value(), 20);
    assert_eq!(receipt.original.put_away.remaining().value(), 20);
    assert_eq!(opening.original.population.original_packs.value(), 8);
    assert_eq!(count.original.counted_packs.value(), 18);
    let pending = db
        .hub_submission(owner(), operation(4))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.kind, "count_observe");
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    assert_eq!(
        format!("{:x}", Sha256::digest(pending.captured_semantics())),
        witness["pending_capture_sha256"].as_str().unwrap()
    );
    let session = db.hub_runtime_session().await.unwrap().unwrap();
    assert_ne!(session.runtime, pending.scope.runtime);
    assert_eq!(session.actor, Some(owner()));
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.position.packs.value())
            .sum::<u32>(),
        28
    );
    let actuals = db
        .hub_actual_reports(owner(), crate::HubActualReportQuery::default())
        .await
        .unwrap();
    assert!(actuals.rows.is_empty());
    assert_eq!(db.hub_runtime_session().await.unwrap(), Some(session));
    db.close().await.unwrap();
    let db = Database::open(&context, 99, 104, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 18);
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(receipt)
    );
    assert_eq!(
        db.hub_opening_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(opening)
    );
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(count)
    );
    assert_eq!(
        db.hub_submission(owner(), operation(4)).await.unwrap(),
        Some(pending.clone())
    );
    assert_eq!(db.hub_runtime_session().await.unwrap(), Some(session));
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    assert_eq!(
        db.hub_actual_reports(owner(), crate::HubActualReportQuery::default())
            .await
            .unwrap(),
        actuals
    );
    db.close().await.unwrap();
    let mut current = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&state)
            .create_if_missing(false)
            .read_only(true),
    )
    .await
    .unwrap();
    assert_eq!(original_rows(&mut current, 17).await, rows17);
    assert_eq!(original_rows(&mut current, 16).await, rows16);
    let after = inventory(&mut current).await;
    assert_eq!(after.as_array().unwrap().len(), 57);
    for table in before.as_array().unwrap() {
        let name = table["table"].as_str().unwrap();
        let observed = after
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["table"] == name)
            .unwrap();
        if !matches!(name, "schema_migrations" | "radroots_service_metadata") {
            assert_eq!(observed, table, "unchanged original table {name}");
        }
    }
    for name in [
        "hub_actual_reports",
        "hub_actual_report_sources",
        "hub_actual_reviews",
        "hub_actual_review_lineage",
        "hub_actual_review_work_scope",
    ] {
        assert_eq!(
            after
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["table"] == name)
                .unwrap()["rows"],
            0
        );
    }
    let current_objects:Vec<(String,String)>=sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(&mut current).await.unwrap();
    assert_eq!(current_objects.len(), 201);
    for (name, sql) in &original_objects {
        if !super::schema::REPLACED_OBJECTS.contains(&name.as_str()) {
            assert_eq!(
                &current_objects.iter().find(|(id, _)| id == name).unwrap().1,
                sql,
                "unchanged original service/host object {name}"
            );
        }
    }
    for object in crate::contract::retained_schema17_objects() {
        if super::schema::REPLACED_OBJECTS.contains(&object.name()) {
            continue;
        }
        let row = sqlx::query("SELECT tbl_name,sql FROM sqlite_schema WHERE name=? LIMIT 1")
            .bind(object.name())
            .fetch_one(&mut current)
            .await
            .unwrap();
        assert_eq!(
            SchemaObject::computed_digest(
                object.kind(),
                object.name(),
                &row.get::<String, _>("tbl_name"),
                &row.get::<String, _>("sql")
            )
            .unwrap(),
            object.digest(),
            "unchanged17 object {}",
            object.name()
        );
    }
    for object in super::schema::schema_objects().unwrap() {
        let row = sqlx::query("SELECT tbl_name,sql FROM sqlite_schema WHERE name=? LIMIT 1")
            .bind(object.name())
            .fetch_one(&mut current)
            .await
            .unwrap();
        assert_eq!(
            SchemaObject::computed_digest(
                object.kind(),
                object.name(),
                &row.get::<String, _>("tbl_name"),
                &row.get::<String, _>("sql")
            )
            .unwrap(),
            object.digest()
        );
    }
    let ledger = sqlx::query("SELECT * FROM schema_migrations ORDER BY version")
        .fetch_all(&mut current)
        .await
        .unwrap();
    assert_eq!(ledger.len(), 17);
    assert_eq!(
        ledger
            .iter()
            .map(|row| row.get::<i64, _>("version"))
            .collect::<Vec<_>>(),
        (2..=18).collect::<Vec<_>>()
    );
    let latest = ledger.last().unwrap();
    assert_eq!(latest.get::<i64, _>("version"), 18);
    assert_eq!(
        latest.get::<String, _>("name"),
        "add_actual_reports_and_reviews"
    );
    assert_eq!(
        latest.get::<Vec<u8>, _>("checksum"),
        super::schema::MIGRATION_CHECKSUM.as_bytes()
    );
    assert_eq!(latest.get::<i64, _>("applied_at_unix_s"), 103);
    assert_eq!(latest.get::<i64, _>("state_contract_version"), 18);
    for (field, expected) in [
        ("service_version", "0.1.0-alpha"),
        ("service_commit", "089dbda0a3a89ed1d4001a1987577d1cdff0f288"),
        ("lib_revision", "189c49b74b4bafc142b00b76b296477931139e72"),
        ("rust_version", "1.99.0"),
        ("target", "aarch64-apple-darwin"),
        (
            "feature_profile",
            "retained-actual-reports-native-upgrade-test",
        ),
    ] {
        assert_eq!(latest.get::<String, _>(field), expected);
    }
    for (field, expected) in [
        ("config_contract_version", 1),
        ("state_contract_version", 18),
        ("admin_contract_version", 1),
        ("status_contract_version", 1),
        ("provider_contract_version", 1),
    ] {
        assert_eq!(latest.get::<i64, _>(field), expected);
    }
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&mut current)
            .await
            .unwrap(),
        "ok"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut current)
            .await
            .unwrap()
            .is_empty()
    );
    current.close().await.unwrap();
    assert_eq!(digest(&archive), ORIGINAL);
    write_new(&result,&serde_json::to_vec_pretty(&serde_json::json!({"schema":18,"actual_predecessor_schema":17,"actual_predecessor_archive_sha256":ORIGINAL,"state_file":state,"original_tables_before":before,"tables_after":after,"pending_operation":pending.operation.as_str(),"pending_capture_sha256":witness["pending_capture_sha256"],"runtime_and_pending_unchanged":true,"native18_reopen_originals_equal":true,"all_migration_build_identity_fields_checked":true,"no_runtime_begin_or_saved_input_execution":true,"personal_identity_or_keyring_data":false})).unwrap());
    println!(
        "Actual17->18 upgraded once; all originals, current runtime and pending claim retained; no saved input executed"
    );
}
