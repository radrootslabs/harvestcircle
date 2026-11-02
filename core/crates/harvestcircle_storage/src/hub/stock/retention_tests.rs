//! Retain the existing schema16 world without opening a writable native host.
use harvestcircle_application::hub_submission::inspect_hub_submission;
use radroots_service_sqlite::SchemaObject;
use sha2::{Digest, Sha256};
use sqlx::{Column, Connection, Row, SqliteConnection, TypeInfo, ValueRef};
use std::{fs, io::Write, path::Path};

const STATE_SHA256: &str = "f31834b6087a01024e6faa3a471fbb1096616a0a83907b4f52dcc0fecd87b28f";
const OLD_SHA256: &str = "5dc7c764c39543e816cb4c02da53e1547c8b0210ddc1f5fdfbecbe589693ebe2";

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

// Includes empty tables, every column and storage type, immutable captures and
// runtime fencing. No advancing column is omitted from this schema16 inventory.
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

fn write_new(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

#[tokio::test]
#[ignore = "Retains the actual closed schema16 world once; never resets, upgrades or executes pending input"]
async fn retain_actual_schema16_before_returns_without_execution() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 16);
    let project = std::path::PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let retained = project.join("retained_evidence").canonicalize().unwrap();
    let predecessor = retained.join("harvestcircle-schema15-before-admission-v1");
    let witness_file = predecessor.join("witness.json");
    let old: serde_json::Value = serde_json::from_slice(&fs::read(&witness_file).unwrap()).unwrap();
    assert_eq!(old["schema"], 15);
    assert_eq!(old["personal_identity_or_keyring_data"], false);
    let state = Path::new(old["state_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let profile = Path::new(old["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let archive15 = Path::new(old["original_archive"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(
        profile.starts_with(&retained)
            && state.starts_with(&profile)
            && archive15.starts_with(&predecessor)
    );
    let output = retained.join("harvestcircle-schema16-before-returns-v1");
    assert!(
        !output.exists(),
        "never overwrite a retained world or partial artifact"
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
        .expect("actual native service writer must be closed");
    let wal = state.with_file_name("state.sqlite-wal");
    assert_eq!(fs::metadata(&wal).unwrap().len(), 0);
    assert_eq!(digest(&state), STATE_SHA256);
    assert_eq!(digest(&archive15), OLD_SHA256);
    let upgrade_log = predecessor.join("admission-schema16-upgrade-1.log");
    let log = fs::read_to_string(&upgrade_log).unwrap();
    assert!(
        log.contains(
            "retained_schema15_admission_upgrade_preserves_originals_and_pending_semantics ... ok"
        ) && log.contains("1 passed; 0 failed")
    );

    let mut raw = reader(&state).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT state_schema_version FROM radroots_service_metadata")
            .fetch_one(&mut raw)
            .await
            .unwrap(),
        16
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
    let objects = crate::contract::retained_schema16_objects();
    let actual_objects: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'")
            .fetch_one(&mut raw)
            .await
            .unwrap();
    // The service catalog excludes the pinned shared host's six metadata/ledger
    // objects. Require exactly those extras and compare their SQL to the exact
    // immutable schema15 archive, rather than ignoring unknown objects.
    const SHARED: [&str; 6] = [
        "radroots_service_metadata",
        "radroots_service_metadata_guard_update",
        "radroots_service_metadata_no_delete",
        "schema_migrations",
        "schema_migrations_no_update",
        "schema_migrations_no_delete",
    ];
    assert_eq!(
        usize::try_from(actual_objects).unwrap(),
        objects.len() + SHARED.len()
    );
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
    let original_rows = super::upgrade_tests::original_rows(&mut raw, 15).await;
    let mut old_raw = reader(&archive15).await;
    for name in SHARED {
        let query = "SELECT type,tbl_name,sql FROM sqlite_schema WHERE name=?";
        let prior = sqlx::query(query)
            .bind(name)
            .fetch_one(&mut old_raw)
            .await
            .unwrap();
        let current = sqlx::query(query)
            .bind(name)
            .fetch_one(&mut raw)
            .await
            .unwrap();
        for column in ["type", "tbl_name", "sql"] {
            assert_eq!(
                prior.get::<String, _>(column),
                current.get::<String, _>(column),
                "shared host object {name}"
            );
        }
    }

    assert_eq!(
        original_rows,
        super::upgrade_tests::original_rows(&mut old_raw, 15).await
    );
    old_raw.close().await.unwrap();
    let submissions = sqlx::query("SELECT * FROM hub_submissions")
        .fetch_all(&mut raw)
        .await
        .unwrap();
    assert_eq!(submissions.len(), 1);
    let pending = &submissions[0];
    let metadata = inspect_hub_submission(&pending.get::<Vec<u8>, _>("capture_body")).unwrap();
    assert_eq!(
        metadata.operation.as_str(),
        "01900000-0000-7000-8000-000000000004"
    );
    assert_eq!(
        metadata.operation.as_str(),
        pending.get::<String, _>("operation_id")
    );
    assert_eq!(metadata.kind, "count_observe");
    assert_eq!(metadata.kind, pending.get::<String, _>("kind"));
    assert_eq!(
        metadata.input_digest.as_slice(),
        pending.get::<Vec<u8>, _>("input_hash")
    );
    assert_eq!(
        metadata.capture_digest.as_slice(),
        pending.get::<Vec<u8>, _>("capture_hash")
    );
    assert_eq!(
        metadata.hub.id().as_bytes().as_slice(),
        pending.get::<Vec<u8>, _>("hub_id")
    );
    assert_eq!(
        metadata.scope.runtime.as_bytes().as_slice(),
        pending.get::<Vec<u8>, _>("runtime_id")
    );
    assert_eq!(
        metadata.scope.generation.value().to_be_bytes().as_slice(),
        pending.get::<Vec<u8>, _>("generation")
    );
    assert_eq!(
        metadata.scope.actor.as_bytes().as_slice(),
        pending.get::<Vec<u8>, _>("actor_public_key")
    );
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
                .bind(metadata.operation.as_str())
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
    let tables = inventory(&mut raw).await;
    raw.close().await.unwrap();
    assert_eq!(digest(&state), STATE_SHA256);
    assert_eq!(fs::metadata(&wal).unwrap().len(), 0);
    fs::create_dir(&output).unwrap();
    let archive = output.join("original-schema16.sqlite");
    write_new(&archive, &fs::read(&state).unwrap());
    assert_eq!(digest(&archive), STATE_SHA256);
    let mut copied = reader(&archive).await;
    assert_eq!(inventory(&mut copied).await, tables);
    copied.close().await.unwrap();
    let witness = serde_json::json!({
        "schema": 16, "original_archive": archive, "original_sha256": STATE_SHA256,
        "state_file": state, "profile_source_root": profile,
        "personal_identity_or_keyring_data": false,
        "predecessor_witness": witness_file, "predecessor_witness_sha256": digest(&witness_file),
        "predecessor_archive_sha256": OLD_SHA256,
        "native_upgrade_log": upgrade_log, "native_upgrade_log_sha256": digest(&upgrade_log),
        "original_rows_through15_sha256": original_rows.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "service_schema_objects": objects.len(), "shared_host_schema_objects": SHARED.len(), "tables": tables,
        "pending_operation": metadata.operation.as_str(), "pending_kind": metadata.kind,
        "pending_capture_sha256": format!("{:x}", Sha256::digest(pending.get::<Vec<u8>, _>("capture_body"))),
        "pending_scope_differs_from_current_runtime": true,
        "purpose": "Read-only retention of actual schema15-to16 world; existing pending input grants no execution or no-effect conclusion",
        "reader_identity": "Current schema16 native contract/archive inspector, not a rebuilt historical producer; source manifest and command log retained separately"
    });
    write_new(
        &output.join("witness.json"),
        &serde_json::to_vec_pretty(&witness).unwrap(),
    );
    assert_eq!(digest(&state), STATE_SHA256);
    assert_eq!(fs::metadata(&wal).unwrap().len(), 0);
    drop(lock);
    println!(
        "retained actual schema16 SHA256={STATE_SHA256}; originals and unresolved submission unchanged; no native host opened"
    );
}
