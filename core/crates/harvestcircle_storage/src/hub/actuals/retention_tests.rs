//! One-use read-only retention of the actual schema18 predecessor.
use crate::hub::stock::upgrade_tests::original_rows;
use radroots_service_sqlite::SchemaObject;
use sha2::{Digest, Sha256};
use sqlx::{Column, Connection, Row, SqliteConnection, TypeInfo, ValueRef};
use std::{fs, io::Write, path::Path};

const PREDECESSOR_SHA256: &str = "e592ac1fa3ea5edb9cf29a55a65078cca4ea32049308d0feebea0be92f9e9c56";
const UPGRADE_RESULT_SHA256: &str =
    "73fc4328f37f90a9449343070ff4842b1fada2a1b3a866b871b5795209c7139e";
const PENDING_CAPTURE_SHA256: &str =
    "dd71489003a223647d2530c8662627bb823c2fb403b565984c3e681c4e48c7fa";
const PENDING_OPERATION: &str = "01900000-0000-7000-8000-000000000004";
const SHARED: [&str; 6] = [
    "radroots_service_metadata",
    "radroots_service_metadata_guard_update",
    "radroots_service_metadata_no_delete",
    "schema_migrations",
    "schema_migrations_no_update",
    "schema_migrations_no_delete",
];

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

async fn reader(path: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false)
            .read_only(true)
            .immutable(true),
    )
    .await
    .unwrap()
}

// Same full-cell inventory used by the consumed native17-to18 qualification.
// Captured input is opaque bytes here; no envelope or saved intent is decoded.
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

fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[tokio::test]
async fn retention_inventory_quotes_names_and_keeps_every_storage_type_and_empty_table() {
    let mut raw = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE \"odd\"\"name\" (n INTEGER, t TEXT, b BLOB, missing TEXT)")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE empty (n INTEGER)")
        .execute(&mut raw)
        .await
        .unwrap();
    sqlx::query("INSERT INTO \"odd\"\"name\" VALUES (7, 'original', x'00FF', NULL)")
        .execute(&mut raw)
        .await
        .unwrap();
    let before = inventory(&mut raw).await;
    assert_eq!(before.as_array().unwrap().len(), 2);
    assert_eq!(before[0]["table"], "empty");
    assert_eq!(before[0]["rows"], 0);
    assert_eq!(before[1]["table"], "odd\"name");
    assert_eq!(before[1]["rows"], 1);
    assert_eq!(inventory(&mut raw).await, before);
    sqlx::query("UPDATE \"odd\"\"name\" SET missing='' ")
        .execute(&mut raw)
        .await
        .unwrap();
    assert_ne!(inventory(&mut raw).await, before);
    raw.close().await.unwrap();
}

#[test]
fn retention_output_never_clobbers_existing_evidence() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("archive");
    write_new(&path, b"original").unwrap();
    assert_eq!(
        write_new(&path, b"replacement").unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(path).unwrap(), b"original");
}

#[tokio::test]
#[ignore = "Retains the actual closed schema18 world once; no upgrade, host opening or saved-input decoding/execution"]
async fn retain_actual_schema18_before_linked_corrections_without_execution() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 18);
    let project = std::path::PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let retained = project.join("retained_evidence").canonicalize().unwrap();
    let predecessor = retained.join("harvestcircle-schema17-before-actuals-v1");
    let witness_file = predecessor.join("witness.json");
    let witness: serde_json::Value =
        serde_json::from_slice(&fs::read(&witness_file).unwrap()).unwrap();
    assert_eq!(witness["schema"], 17);
    assert_eq!(witness["personal_identity_or_keyring_data"], false);
    let result_file = predecessor.join("actual-schema18-upgrade-1-result.json");
    assert_eq!(digest(&result_file), UPGRADE_RESULT_SHA256);
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(&result_file).unwrap()).unwrap();
    assert_eq!(result["schema"], 18);
    for field in [
        "runtime_and_pending_unchanged",
        "native18_reopen_originals_equal",
        "all_migration_build_identity_fields_checked",
        "no_runtime_begin_or_saved_input_execution",
    ] {
        assert_eq!(result[field], true);
    }
    assert_eq!(result["personal_identity_or_keyring_data"], false);
    let declared = Path::new(result["state_file"].as_str().unwrap());
    let state = declared.canonicalize().unwrap();
    assert_eq!(state, declared);
    assert!(fs::symlink_metadata(&state).unwrap().file_type().is_file());
    let profile = Path::new(witness["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let archive17 = Path::new(witness["original_archive"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert_eq!(
        state,
        Path::new(witness["state_file"].as_str().unwrap())
            .canonicalize()
            .unwrap()
    );
    assert!(
        profile.starts_with(&retained)
            && state.starts_with(&profile)
            && archive17.starts_with(&predecessor)
    );
    assert_eq!(digest(&archive17), PREDECESSOR_SHA256);
    let output = retained.join("harvestcircle-schema18-before-corrections-v1");
    assert!(
        !output.exists(),
        "never overwrite retained or partial evidence"
    );
    let lock_path = state.with_file_name("state.lock");
    assert!(
        !fs::symlink_metadata(&lock_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let lock = fs::File::open(&lock_path).unwrap();
    lock.try_lock()
        .expect("actual native writer must be closed");
    let wal = state.with_file_name("state.sqlite-wal");
    assert_eq!(fs::metadata(&wal).unwrap().len(), 0);
    let original_sha256 = digest(&state);
    let mut raw = reader(&state).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT state_schema_version FROM radroots_service_metadata")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        18
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        "ok"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut raw)
            .await
            .unwrap()
            .is_empty()
    );
    let mut objects = crate::contract::retained_schema17_objects();
    objects.retain(|o| !super::schema::REPLACED_OBJECTS.contains(&o.name()));
    objects.extend(super::schema::schema_objects().unwrap());
    let catalog = crate::contract::harvestcircle_schema_catalog().unwrap();
    let native18 = catalog
        .versions()
        .iter()
        .find(|v| v.version() == 18)
        .unwrap();
    assert_eq!(
        radroots_service_sqlite::SchemaVersionCatalog::computed_digest(18, objects.clone())
            .unwrap(),
        native18.digest()
    );
    let actual_objects: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'")
            .fetch_one(&mut raw)
            .await
            .unwrap();
    assert_eq!(
        usize::try_from(actual_objects).unwrap(),
        objects.len() + SHARED.len()
    );
    assert_eq!(actual_objects, 201);
    for object in &objects {
        let row = sqlx::query("SELECT tbl_name,sql FROM sqlite_schema WHERE name=?")
            .bind(object.name())
            .fetch_one(&mut raw)
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
            "{}",
            object.name()
        );
    }
    let mut old = reader(&archive17).await;
    for name in SHARED {
        let prior = sqlx::query("SELECT type,tbl_name,sql FROM sqlite_schema WHERE name=?")
            .bind(name)
            .fetch_one(&mut old)
            .await
            .unwrap();
        let current = sqlx::query("SELECT type,tbl_name,sql FROM sqlite_schema WHERE name=?")
            .bind(name)
            .fetch_one(&mut raw)
            .await
            .unwrap();
        for column in ["type", "tbl_name", "sql"] {
            assert_eq!(
                prior.get::<String, _>(column),
                current.get::<String, _>(column),
                "shared host {name}"
            );
        }
    }
    let original_rows17 = original_rows(&mut raw, 17).await;
    assert_eq!(original_rows17, original_rows(&mut old, 17).await);
    old.close().await.unwrap();
    let tables = inventory(&mut raw).await;
    assert_eq!(tables.as_array().unwrap().len(), 57);
    assert_eq!(
        tables, result["tables_after"],
        "all actual18 cells must match the consumed upgrade result"
    );
    let submissions = sqlx::query("SELECT * FROM hub_submissions")
        .fetch_all(&mut raw)
        .await
        .unwrap();
    assert_eq!(submissions.len(), 1);
    let pending = &submissions[0];
    assert_eq!(pending.get::<String, _>("operation_id"), PENDING_OPERATION);
    assert_eq!(pending.get::<String, _>("kind"), "count_observe");
    let capture = pending.get::<Vec<u8>, _>("capture_body");
    assert_eq!(
        format!("{:x}", Sha256::digest(&capture)),
        PENDING_CAPTURE_SHA256
    );
    assert_eq!(result["pending_capture_sha256"], PENDING_CAPTURE_SHA256);
    assert_eq!(result["pending_operation"], PENDING_OPERATION);
    let current_runtime: Vec<u8> = sqlx::query_scalar("SELECT runtime_id FROM hub_runtime_session")
        .fetch_one(&mut raw)
        .await
        .unwrap();
    assert_ne!(current_runtime, pending.get::<Vec<u8>, _>("runtime_id"));
    for query in [
        "SELECT count(*) FROM hub_history WHERE operation_id=?",
        "SELECT count(*) FROM hub_reference_versions WHERE operation_id=?",
        "SELECT count(*) FROM hub_stock_operations WHERE operation_id=?",
        "SELECT count(*) FROM hub_submission_rejections WHERE operation_id=?",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(query)
                .bind(PENDING_OPERATION)
                .fetch_one(&mut raw)
                .await
                .unwrap(),
            0
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT sum(packs) FROM hub_stock_positions")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        28
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT sum(scoped_packs) FROM hub_group_work WHERE progress='pending'"
        )
        .fetch_one(&mut raw)
        .await
        .unwrap(),
        20
    );
    let ledger = sqlx::query("SELECT * FROM schema_migrations ORDER BY version")
        .fetch_all(&mut raw)
        .await
        .unwrap();
    assert_eq!(
        ledger
            .iter()
            .map(|r| r.get::<i64, _>("version"))
            .collect::<Vec<_>>(),
        (2..=18).collect::<Vec<_>>()
    );
    let last = ledger.last().unwrap();
    assert_eq!(
        last.get::<String, _>("name"),
        "add_actual_reports_and_reviews"
    );
    assert_eq!(
        last.get::<Vec<u8>, _>("checksum"),
        super::schema::MIGRATION_CHECKSUM.as_bytes()
    );
    assert_eq!(last.get::<i64, _>("applied_at_unix_s"), 103);
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
        assert_eq!(last.get::<String, _>(field), expected);
    }
    for (field, expected) in [
        ("config_contract_version", 1),
        ("state_contract_version", 18),
        ("admin_contract_version", 1),
        ("status_contract_version", 1),
        ("provider_contract_version", 1),
    ] {
        assert_eq!(last.get::<i64, _>(field), expected);
    }
    raw.close().await.unwrap();
    assert_eq!(digest(&state), original_sha256);
    assert_eq!(fs::metadata(&wal).unwrap().len(), 0);
    fs::create_dir(&output).unwrap();
    write_new(
        &output.join("retention.claim"),
        b"Actual schema18 read-only retention; never rerun this collector\n",
    )
    .unwrap();
    let archive = output.join("original-schema18.sqlite");
    let bytes = fs::read(&state).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), original_sha256);
    write_new(&archive, &bytes).unwrap();
    assert_eq!(digest(&archive), original_sha256);
    let mut copied = reader(&archive).await;
    assert_eq!(inventory(&mut copied).await, tables);
    copied.close().await.unwrap();
    let witness = serde_json::json!({"schema":18,"original_archive":archive,"original_sha256":original_sha256,"state_file":state,"profile_source_root":profile,"personal_identity_or_keyring_data":false,"predecessor_witness":witness_file,"predecessor_witness_sha256":digest(&witness_file),"predecessor_archive_sha256":PREDECESSOR_SHA256,"native_upgrade_result":result_file,"native_upgrade_result_sha256":UPGRADE_RESULT_SHA256,"original_rows_through17_sha256":original_rows17.iter().map(|b|format!("{b:02x}")).collect::<String>(),"service_schema_objects":objects.len(),"shared_host_schema_objects":SHARED.len(),"tables":tables,"pending_operation":PENDING_OPERATION,"pending_kind":"count_observe","pending_capture_sha256":PENDING_CAPTURE_SHA256,"pending_scope_differs_from_current_runtime":true,"saved_input_decoded_or_executed_by_inspector":false,"absence_is_no_effect_proof":false,"purpose":"Read-only retention of actual18 before linked corrections; pending input remains unresolved and grants no execution","reader_identity":"Current native18 schema/archive inspector, no writable service host or runtime begin; qualified source/binary/command identities retained separately"});
    write_new(
        &output.join("witness.json"),
        &serde_json::to_vec_pretty(&witness).unwrap(),
    )
    .unwrap();
    assert_eq!(digest(&state), original_sha256);
    assert_eq!(fs::metadata(&wal).unwrap().len(), 0);
    assert_eq!(digest(&archive17), PREDECESSOR_SHA256);
    drop(lock);
    println!(
        "Retained actual schema18 SHA256={original_sha256}; all57tables/201objects/originals and unresolved capture unchanged; no native host opened"
    );
}
