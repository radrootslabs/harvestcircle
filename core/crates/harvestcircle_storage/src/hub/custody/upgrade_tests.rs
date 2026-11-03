//! One-use consumer of the actual retained schema16 world, never a synthesized donor.
use crate::Database;
use crate::hub::stock::{
    tests::{now, operation, owner, query},
    upgrade_tests::original_rows,
};
use harvestcircle_application::{
    SessionGeneration,
    hub_submission::{HubMutation, HubSubmissionScope},
};
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{
        disposition::{DispositionCommand, DispositionKind},
        movement::PhysicalIdentification,
        reference::HubLabel,
        reference::{SourceAttribution, SourceProvenance, WholePacks},
        returns::ReturnCommand,
        stock::Occurrence,
    },
};
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use sha2::{Digest, Sha256};
use sqlx::{
    Column, Connection, Row, SqliteConnection, TypeInfo, ValueRef, sqlite::SqliteConnectOptions,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const ORIGINAL: &str = "f31834b6087a01024e6faa3a471fbb1096616a0a83907b4f52dcc0fecd87b28f";
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

async fn unchanged(raw: &mut SqliteConnection, before: &serde_json::Value, originals: [u8; 32]) {
    assert_eq!(original_rows(raw, 16).await, originals);
    let after = inventory(raw).await;
    let before = before.as_array().unwrap();
    let after = after.as_array().unwrap();
    for table in before {
        let name = table["table"].as_str().unwrap();
        let actual = after.iter().find(|r| r["table"] == name).unwrap();
        // Native schema number advances and one new migration ledger row is added.
        // All their predecessor columns/rows are checked separately by original_rows16.
        if !matches!(name, "schema_migrations" | "radroots_service_metadata") {
            assert_eq!(actual, table, "{name}");
        }
    }
    assert_eq!(before.len(), 48);
    assert_eq!(after.len(), 52);
    for name in [
        "hub_returns",
        "hub_return_sources",
        "hub_dispositions",
        "hub_disposition_work_scope",
    ] {
        assert_eq!(
            after.iter().find(|r| r["table"] == name).unwrap()["rows"],
            0
        );
    }
    let ledger = sqlx::query("SELECT * FROM schema_migrations ORDER BY version")
        .fetch_all(&mut *raw)
        .await
        .unwrap();
    assert_eq!(ledger.len(), 16);
    assert_eq!(
        ledger
            .iter()
            .map(|r| r.get::<i64, _>("version"))
            .collect::<Vec<_>>(),
        (2..=17).collect::<Vec<_>>()
    );
    let current = ledger.last().unwrap();
    assert_eq!(
        current.get::<String, _>("name"),
        "add_returned_custody_and_dispositions"
    );
    assert_eq!(
        current.get::<Vec<u8>, _>("checksum"),
        super::schema::MIGRATION_CHECKSUM.as_bytes()
    );
    assert_eq!(current.get::<i64, _>("applied_at_unix_s"), 102);
    for (field, expected) in [
        ("service_version", "0.1.0-alpha"),
        ("service_commit", "5555555555555555555555555555555555555555"),
        ("lib_revision", "189c49b74b4bafc142b00b76b296477931139e72"),
        ("rust_version", "1.99.0"),
        ("target", "test"),
        ("feature_profile", "test"),
    ] {
        assert_eq!(current.get::<String, _>(field), expected);
    }
    for (field, expected) in [
        ("config_contract_version", 1),
        ("state_contract_version", 17),
        ("admin_contract_version", 1),
        ("status_contract_version", 1),
        ("provider_contract_version", 1),
    ] {
        assert_eq!(current.get::<i64, _>(field), expected);
    }
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

#[tokio::test]
#[ignore = "Consumes actual retained schema16 exactly once; never resets originals or executes saved pending input"]
async fn retained_actual_schema16_custody_upgrade_keeps_every_original_and_unresolved_operation() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 17);
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let retained = project.join("retained_evidence").canonicalize().unwrap();
    let output = retained
        .join("harvestcircle-schema16-before-returns-v1")
        .canonicalize()
        .unwrap();
    assert_eq!(output.parent(), Some(retained.as_path()));
    let witness_file = output.join("witness.json");
    let witness: serde_json::Value =
        serde_json::from_slice(&fs::read(&witness_file).unwrap()).unwrap();
    assert_eq!(witness["schema"], 16);
    assert_eq!(witness["personal_identity_or_keyring_data"], false);
    assert_eq!(witness["original_sha256"], ORIGINAL);
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
    assert_eq!(
        digest(&state),
        ORIGINAL,
        "actual16 must not already be consumed or altered"
    );
    assert_eq!(
        fs::metadata(state.with_file_name("state.sqlite-wal"))
            .unwrap()
            .len(),
        0
    );
    let options = SqliteConnectOptions::new()
        .filename(&state)
        .create_if_missing(false)
        .read_only(true);
    let mut raw = SqliteConnection::connect_with(&options.clone().immutable(true))
        .await
        .unwrap();
    let before = inventory(&mut raw).await;
    assert_eq!(before, witness["tables"]);
    assert_eq!(before.as_array().unwrap().len(), 48);
    let originals = original_rows(&mut raw, 16).await;
    raw.close().await.unwrap();
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
        "5555555555555555555555555555555555555555",
        "189c49b74b4bafc142b00b76b296477931139e72",
        "1.99.0",
        "test",
        "test",
        1,
        17,
        1,
        1,
        1,
    )
    .unwrap();
    let contract = crate::HarvestCircleStorageContract::from_runtime_context(&context).unwrap();
    assert_eq!(contract.paths().state_database(), state.as_path());
    assert!(!output.join("custody-schema17-upgrade-1.claim").exists());
    assert!(
        !output
            .join("custody-schema17-upgrade-1-result.json")
            .exists()
    );
    // The marker precedes the first writable native open. Any failure after here
    // requires reconciliation; never rerun, replace or remove this claim.
    write_new(
        &output.join("custody-schema17-upgrade-1.claim"),
        b"One-use actual16->17 native consumer; preserve any partial state.\n",
    );
    let db = Database::open(&context, 99, 102, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 17);
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
    assert_eq!(session.actor, Some(owner()));
    assert_ne!(session.runtime, pending.scope.runtime);
    assert_ne!(session.generation, SessionGeneration::initial());
    let scope = HubSubmissionScope {
        runtime: session.runtime,
        generation: session.generation,
        actor: owner(),
    };
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        28
    );
    let catalog = db.hub_references(owner()).await.unwrap();
    let group = db
        .hub_group(owner(), receipt.original.position.group)
        .await
        .unwrap();
    let note = || {
        SourceProvenance::new(
            "Fresh test-owned request must stay blocked by retained unknown prior outcome",
        )
        .unwrap()
    };
    let returned = ReturnCommand {
        expected_stock_revision: page.revision,
        expected_catalog_revision: catalog.revision(),
        variant: receipt.original.trace.variant.id,
        location: receipt.original.trace.initial_location.id,
        returned_by: None,
        source: SourceAttribution::Unknown,
        packs: WholePacks::from_value(2).unwrap(),
        identification: PhysicalIdentification {
            label: HubLabel::new("Blocked fresh return").unwrap(),
            observation: note(),
        },
        planned_location: None,
        departure: None,
        occurred: Occurrence::Unknown,
        observation: note(),
    };
    let disposed = DispositionCommand {
        expected_stock_revision: page.revision,
        expected_catalog_revision: catalog.revision(),
        group: group.group.id,
        expected_group_revision: group.group.revision,
        packs: group.group.packs,
        kind: DispositionKind::Destroyed,
        occurred: Occurrence::Unknown,
        observation: note(),
    };
    for (op, request) in [
        (1007, HubMutation::Return(Box::new(returned))),
        (1008, HubMutation::Disposition(Box::new(disposed))),
    ] {
        assert_eq!(
            db.submit_hub_mutation(scope, operation(op), request, now())
                .await
                .unwrap_err()
                .code(),
            SafeErrorCode::HubConflict
        );
        assert!(
            db.hub_submission(owner(), operation(op))
                .await
                .unwrap()
                .is_none()
        );
    }
    // No saved pending command was decoded, replayed, rejected or cleared.
    assert_eq!(
        db.hub_submission(owner(), operation(4))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(db.hub_runtime_session().await.unwrap(), Some(session));
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    unchanged(&mut raw, &before, originals).await;
    raw.close().await.unwrap();
    let db = Database::open(&context, 99, 103, &build).await.unwrap();
    assert_eq!(
        db.hub_submission(owner(), operation(4))
            .await
            .unwrap()
            .unwrap(),
        pending
    );
    assert_eq!(db.hub_runtime_session().await.unwrap(), Some(session));
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
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    unchanged(&mut raw, &before, originals).await;
    raw.close().await.unwrap();
    assert_eq!(digest(&archive), ORIGINAL);
    write_new(&output.join("custody-schema17-upgrade-1-result.json"),&serde_json::to_vec_pretty(&serde_json::json!({
        "original_schema":16,"current_schema":17,"original_sha256":ORIGINAL,"archive_sha256_after":digest(&archive),
        "original_rows_through16_sha256":originals.iter().map(|b| format!("{b:02x}")).collect::<String>(),"original_table_inventory":before,
        "pending_operation":operation(4).as_str(),"pending_capture_sha256":witness["pending_capture_sha256"],"pending_status":"pending",
        "saved_input_executed":false,"new_kinds_blocked_without_admission":true,"runtime_session_unchanged":true,"reopen_originals_equal":true,
        "integrity":"ok","foreign_keys":"ok"
    })).unwrap());
}
