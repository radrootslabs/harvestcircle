//! One-use actual18→19 consumer and retention of its published19 successor.
//! Pending input remains opaque; no runtime begin, submission or historical replay.
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

const ORIGINAL: &str = "9efe167273cb9227f3cc0cd72d8df2a8436d6862f24e3e483f8bbcd83b2ed7fc";
const WITNESS: &str = "d625ab673729b94ac45d296e32842d536ee8100e32638352d5fa1966fc4729f5";
const CAPTURE: &str = "dd71489003a223647d2530c8662627bb823c2fb403b565984c3e681c4e48c7fa";
const SOURCE: &str = "8c59793dbbd83cb58f420682a865bc2197c515de";
const LIB: &str = "189c49b74b4bafc142b00b76b296477931139e72";
const FEATURE: &str = "retained-linked-corrections-native-upgrade-test";

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
// Full native storage types, column names, row ordering and empty tables. This
// inspector never deserializes capture_body or treats a missing result as proof.
async fn inventory(raw: &mut SqliteConnection) -> serde_json::Value {
    let names = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    ).fetch_all(&mut *raw).await.unwrap();
    let mut tables = Vec::new();
    for name in names {
        let query = format!(
            "SELECT * FROM \"{}\" ORDER BY rowid",
            name.replace('"', "\"\"")
        );
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
        tables.push(serde_json::json!({"table":name,"rows":rows.len(),"sha256":format!("{:x}",hash.finalize())}));
    }
    serde_json::Value::Array(tables)
}
async fn verify_objects(raw: &mut SqliteConnection, objects: &[SchemaObject]) {
    for object in objects {
        let row = sqlx::query("SELECT tbl_name,sql FROM sqlite_schema WHERE name=?")
            .bind(object.name())
            .fetch_one(&mut *raw)
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
            "published object {}",
            object.name()
        );
    }
}
async fn all_objects(raw: &mut SqliteConnection) -> Vec<(String, String)> {
    sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .fetch_all(&mut *raw).await.unwrap()
}
async fn health(raw: &mut SqliteConnection) {
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&mut *raw)
            .await
            .unwrap(),
        "ok"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *raw)
            .await
            .unwrap()
            .is_empty()
    );
}

fn published19_objects() -> Vec<SchemaObject> {
    let mut objects = crate::contract::retained_schema18_objects();
    assert_eq!(objects.len(), 195);
    assert_eq!(super::schema::REPLACED_OBJECTS.len(), 2);
    for name in super::schema::REPLACED_OBJECTS {
        assert_eq!(objects.iter().filter(|o| o.name() == *name).count(), 1);
    }
    objects.retain(|object| !super::schema::REPLACED_OBJECTS.contains(&object.name()));
    assert_eq!(objects.len(), 193);
    let forward = super::schema::schema_objects().unwrap();
    assert_eq!(forward.len(), 63);
    for name in super::schema::REPLACED_OBJECTS {
        assert_eq!(forward.iter().filter(|o| o.name() == *name).count(), 1);
    }
    objects.extend(forward);
    assert_eq!(objects.len(), 256);
    let catalog = crate::harvestcircle_schema_catalog().unwrap();
    let native19 = catalog
        .versions()
        .iter()
        .find(|v| v.version() == 19)
        .unwrap();
    assert_eq!(
        radroots_service_sqlite::SchemaVersionCatalog::computed_digest(19, objects.clone())
            .unwrap(),
        native19.digest()
    );
    objects
}

#[test]
fn published19_inspector_inventory_matches_registered_native_catalog_before_any_claim() {
    published19_objects();
}

#[tokio::test]
#[ignore = "Consumes actual retained18 once and retains19; never reset, begin runtime or execute pending input"]
async fn retained_actual18_upgrade_preserves_every_original_and_retains_published19() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 19);
    assert_eq!(
        (std::env::consts::OS, std::env::consts::ARCH),
        ("macos", "aarch64")
    );
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let retained = project.join("retained_evidence").canonicalize().unwrap();
    let input = retained
        .join("harvestcircle-schema18-before-corrections-v1")
        .canonicalize()
        .unwrap();
    assert_eq!(input.parent(), Some(retained.as_path()));
    let witness_file = input.join("witness.json");
    assert_eq!(digest(&witness_file), WITNESS);
    let witness: serde_json::Value =
        serde_json::from_slice(&fs::read(&witness_file).unwrap()).unwrap();
    assert_eq!(witness["schema"], 18);
    assert_eq!(witness["original_sha256"], ORIGINAL);
    assert_eq!(witness["pending_capture_sha256"], CAPTURE);
    assert_eq!(witness["personal_identity_or_keyring_data"], false);
    let state = PathBuf::from(witness["state_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let archive = PathBuf::from(witness["original_archive"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let profile = PathBuf::from(witness["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(
        profile.starts_with(&retained)
            && state.starts_with(&profile)
            && archive.starts_with(&input)
    );
    assert_eq!(digest(&archive), ORIGINAL);
    assert_eq!(digest(&state), ORIGINAL);
    assert_eq!(
        fs::metadata(state.with_extension("sqlite-wal"))
            .unwrap()
            .len(),
        0
    );
    let objects18 = crate::contract::retained_schema18_objects();
    let objects19 = published19_objects();
    assert_eq!(objects18.len(), 195);
    let mut old = reader(&archive).await;
    verify_objects(&mut old, &objects18).await;
    health(&mut old).await;
    let before = inventory(&mut old).await;
    assert_eq!(before, witness["tables"]);
    assert_eq!(before.as_array().unwrap().len(), 57);
    let old_objects = all_objects(&mut old).await;
    assert_eq!(old_objects.len(), 201);
    let originals18 = original_rows(&mut old, 18).await;
    let originals17 = original_rows(&mut old, 17).await;
    old.close().await.unwrap();
    let mut raw = reader(&state).await;
    verify_objects(&mut raw, &objects18).await;
    assert_eq!(inventory(&mut raw).await, before);
    raw.close().await.unwrap();
    let output = retained.join("harvestcircle-schema19-before-actual-interpretations-v1");
    let claim = input.join("actual-schema19-upgrade-1.claim");
    assert!(!output.exists() && !claim.exists());
    let context = RuntimeContext::resolve(
        &RadrootsPathResolver::new(
            RadrootsPlatform::current(),
            RadrootsHostEnvironment::default(),
        ),
        RuntimeContextBootstrap::new(
            RadrootsPathProfile::RepoLocal,
            Some(profile.clone()),
            RuntimeContextSource::BootstrapCli,
            RuntimeContextSource::SafeDefault,
        )
        .unwrap(),
        ServiceId::new("harvestcircle").unwrap(),
        InstanceId::new("desktop").unwrap(),
    )
    .unwrap();
    let contract = crate::HarvestCircleStorageContract::from_runtime_context(&context).unwrap();
    assert_eq!(contract.paths().state_database(), state.as_path());
    let build = MigrationBuildIdentity::new(
        "0.1.0-alpha",
        SOURCE,
        LIB,
        "1.99.0",
        "aarch64-apple-darwin",
        FEATURE,
        1,
        19,
        1,
        1,
        1,
    )
    .unwrap();
    // No writable host before this permanent create-new claim. A partial failure
    // consumes the attempt too: retain every byte/lock and reconcile, never rerun.
    write_new(
        &claim,
        b"One-use actual18->19 native upgrade and published19 retention.\n",
    );
    fs::create_dir(&output).unwrap();
    let db = Database::open(&context, 99, 106, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 19);
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
    assert_eq!(pending.status, crate::HubSubmissionStatus::Pending);
    assert_eq!(pending.kind, "count_observe");
    assert_eq!(
        format!("{:x}", Sha256::digest(pending.captured_semantics())),
        CAPTURE
    );
    let session = db.hub_runtime_session().await.unwrap().unwrap();
    assert_ne!(session.runtime, pending.scope.runtime);
    assert_eq!(session.actor, Some(owner()));
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        28
    );
    let reports = db
        .hub_actual_reports(owner(), crate::HubActualReportQuery::default())
        .await
        .unwrap();
    assert!(reports.rows.is_empty());
    db.close().await.unwrap();
    let db = Database::open(&context, 99, 107, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 19);
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
        Some(pending)
    );
    assert_eq!(db.hub_runtime_session().await.unwrap(), Some(session));
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    assert_eq!(
        db.hub_actual_reports(owner(), crate::HubActualReportQuery::default())
            .await
            .unwrap(),
        reports
    );
    db.close().await.unwrap();
    assert!(read_only_recovery::closed_wal(&state));
    let mut current = reader(&state).await;
    assert_eq!(original_rows(&mut current, 18).await, originals18);
    assert_eq!(original_rows(&mut current, 17).await, originals17);
    let after = inventory(&mut current).await;
    assert_eq!(after.as_array().unwrap().len(), 69);
    for table in before.as_array().unwrap() {
        let name = table["table"].as_str().unwrap();
        let observed = after
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["table"] == name)
            .unwrap();
        if !matches!(name, "schema_migrations" | "radroots_service_metadata") {
            assert_eq!(observed, table, "unchanged original {name}");
        }
    }
    for table in after.as_array().unwrap() {
        if !before
            .as_array()
            .unwrap()
            .iter()
            .any(|old| old["table"] == table["table"])
        {
            assert_eq!(table["rows"], 0);
        }
    }
    verify_objects(&mut current, &objects19).await;
    let new_objects = all_objects(&mut current).await;
    assert_eq!(new_objects.len(), objects19.len() + 6);
    for (name, sql) in &old_objects {
        if !super::schema::REPLACED_OBJECTS.contains(&name.as_str()) {
            assert_eq!(
                &new_objects.iter().find(|(n, _)| n == name).unwrap().1,
                sql,
                "unchanged old service/host object {name}"
            );
        }
    }
    let ledger = sqlx::query("SELECT * FROM schema_migrations ORDER BY version")
        .fetch_all(&mut current)
        .await
        .unwrap();
    assert_eq!(
        ledger
            .iter()
            .map(|r| r.get::<i64, _>("version"))
            .collect::<Vec<_>>(),
        (2..=19).collect::<Vec<_>>()
    );
    let latest = ledger.last().unwrap();
    assert_eq!(
        latest.get::<String, _>("name"),
        "add_linked_correction_ledger"
    );
    assert_eq!(
        latest.get::<Vec<u8>, _>("checksum"),
        super::schema::MIGRATION_CHECKSUM.as_bytes()
    );
    assert_eq!(latest.get::<i64, _>("applied_at_unix_s"), 106);
    for (field, value) in [
        ("service_version", "0.1.0-alpha"),
        ("service_commit", SOURCE),
        ("lib_revision", LIB),
        ("rust_version", "1.99.0"),
        ("target", "aarch64-apple-darwin"),
        ("feature_profile", FEATURE),
    ] {
        assert_eq!(latest.get::<String, _>(field), value);
    }
    for (field, value) in [
        ("config_contract_version", 1),
        ("state_contract_version", 19),
        ("admin_contract_version", 1),
        ("status_contract_version", 1),
        ("provider_contract_version", 1),
    ] {
        assert_eq!(latest.get::<i64, _>(field), value);
    }
    health(&mut current).await;
    current.close().await.unwrap();
    assert_eq!(digest(&archive), ORIGINAL);
    let state_hash = digest(&state);
    let archive19 = output.join("original-schema19.sqlite");
    write_new(&archive19, &fs::read(&state).unwrap());
    assert_eq!(digest(&archive19), state_hash);
    let mut copied = reader(&archive19).await;
    assert_eq!(inventory(&mut copied).await, after);
    verify_objects(&mut copied, &objects19).await;
    health(&mut copied).await;
    copied.close().await.unwrap();
    assert_eq!(digest(&state), state_hash);
    let result = serde_json::json!({"schema":19,"actual_predecessor_schema":18,
        "predecessor_witness":witness_file,"predecessor_witness_sha256":digest(&witness_file),
        "actual_predecessor_archive_sha256":ORIGINAL,"state_file":state,"profile_source_root":profile,
        "original_archive":archive19,"original_sha256":state_hash,"tables":after,
        "original_tables_before":before,"service_schema_objects":objects19.len(),"shared_host_schema_objects":6,
        "schema_objects":new_objects,"pending_operation":operation(4).as_str(),"pending_capture_sha256":CAPTURE,
        "runtime_and_pending_unchanged":true,"native19_reopen_originals_equal":true,
        "all_migration_build_identity_fields_checked":true,"source_commit":SOURCE,"source_harness_may_be_uncommitted":true,
        "no_runtime_begin_or_saved_input_execution":true,"personal_identity_or_keyring_data":false,
        "absence_is_no_effect_proof":false,"upgrade_claim":claim,"consumed_do_not_rerun":true});
    write_new(
        &output.join("witness.json"),
        &serde_json::to_vec_pretty(&result).unwrap(),
    );
    println!(
        "Actual18->19 consumed once; all originals/runtime/pending preserved; published19 archive retained"
    );
}

mod read_only_recovery;
