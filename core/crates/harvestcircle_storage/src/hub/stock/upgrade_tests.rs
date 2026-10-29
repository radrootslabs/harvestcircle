//! Explicit consumption of an actual, closed pre-migration fictional world.
use super::tests::{command, now, operation, owner, query};
use crate::Database;
use harvestcircle_domain::hub::stock::PopulationOrigin;
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use sha2::{Digest, Sha256};
use sqlx::{
    Column, Connection, Row, SqliteConnection, TypeInfo, ValueRef, sqlite::SqliteConnectOptions,
};
use std::path::PathBuf;

const ORIGINAL_SHA256: &str = "acf88350ab7bf2db5bcbd1aea28562a9de99ba8a82a218775e70203456b01731";

// Every nonempty original table is covered, excluding only the advancing schema
// version column and newly appended migration row. No original is reconstructed.
pub(in crate::hub) async fn original_rows(
    connection: &mut SqliteConnection,
    through_version: u32,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    for query in [
        "SELECT * FROM hub_context ORDER BY rowid",
        "SELECT * FROM hub_group_identification ORDER BY rowid",
        "SELECT * FROM hub_group_work ORDER BY rowid",
        "SELECT * FROM hub_handling_context ORDER BY rowid",
        "SELECT * FROM hub_history ORDER BY rowid",
        "SELECT * FROM hub_receipts ORDER BY rowid",
        "SELECT * FROM hub_reference_versions ORDER BY rowid",
        "SELECT * FROM hub_stock_events ORDER BY rowid",
        "SELECT * FROM hub_stock_groups ORDER BY rowid",
        "SELECT * FROM hub_stock_operations ORDER BY rowid",
        "SELECT * FROM hub_stock_positions ORDER BY rowid",
        "SELECT * FROM hub_stock_state ORDER BY rowid",
        "SELECT * FROM hub_work_requirements ORDER BY rowid",
        "SELECT * FROM public_payload_usage ORDER BY rowid",
        "SELECT * FROM radroots_service_metadata ORDER BY rowid",
        "SELECT * FROM runtime_state ORDER BY rowid",
        "SELECT * FROM schema_migrations WHERE version <= ? ORDER BY version",
    ]
    .into_iter()
    .chain((through_version >= 13).then_some("SELECT * FROM hub_counted_openings ORDER BY rowid"))
    .chain((through_version >= 14).then_some("SELECT * FROM hub_count_observations ORDER BY rowid"))
    .chain(
        (through_version >= 15).then_some("SELECT * FROM hub_count_reconciliations ORDER BY rowid"),
    )
    .chain(
        (through_version >= 15)
            .then_some("SELECT * FROM hub_count_reconciliation_work ORDER BY rowid"),
    )
    .chain((through_version >= 15).then_some("SELECT * FROM hub_counted_surplus ORDER BY rowid"))
    {
        hash.update(query.as_bytes());
        let statement = sqlx::query(query);
        let statement =
            if query == "SELECT * FROM schema_migrations WHERE version <= ? ORDER BY version" {
                statement.bind(i64::from(through_version))
            } else {
                statement
            };
        for row in statement.fetch_all(&mut *connection).await.unwrap() {
            hash.update(b"row\0");
            for column in row.columns() {
                if column.name() == "state_schema_version"
                    || (through_version < 12 && column.name() == "origin_kind")
                {
                    continue;
                }
                hash.update(column.name().as_bytes());
                let value = row.try_get_raw(column.ordinal()).unwrap();
                let bytes = if value.is_null() {
                    hash.update(b"null\0");
                    Vec::new()
                } else {
                    let kind = value.type_info();
                    hash.update(kind.name().as_bytes());
                    match kind.name() {
                        "INTEGER" => row.get::<i64, _>(column.ordinal()).to_be_bytes().to_vec(),
                        "TEXT" => row.get::<String, _>(column.ordinal()).into_bytes(),
                        "BLOB" => row.get::<Vec<u8>, _>(column.ordinal()),
                        kind => panic!("unexpected original fixture storage type: {kind}"),
                    }
                };
                hash.update(u64::try_from(bytes.len()).unwrap().to_be_bytes());
                hash.update(bytes);
            }
        }
    }
    hash.finalize().into()
}

#[tokio::test]
#[ignore = "Consumes the owned actual schema11 witness; never creates or resets an old world"]
async fn retained_schema11_population_upgrade_preserves_every_original_and_replay() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 12);
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let output = project
        .join("retained_evidence/harvestcircle-schema11-before-counts-v1")
        .canonicalize()
        .unwrap();
    assert_eq!(output.parent().unwrap().parent(), Some(project.as_path()));
    let witness: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("witness.json")).unwrap()).unwrap();
    assert_eq!(witness["schema"], 11);
    let archive = PathBuf::from(witness["original_archive"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let profile = PathBuf::from(witness["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let state = PathBuf::from(witness["state_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(
        archive.starts_with(&output) && profile.starts_with(&output) && state.starts_with(&profile)
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&archive).unwrap())),
        ORIGINAL_SHA256
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&state).unwrap())),
        ORIGINAL_SHA256,
        "the original owned profile must still be the closed schema11 world"
    );
    let options = SqliteConnectOptions::new()
        .filename(&state)
        .create_if_missing(false)
        .read_only(true);
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    let before = original_rows(&mut raw, 11).await;
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
        "2222222222222222222222222222222222222222",
        "189c49b74b4bafc142b00b76b296477931139e72",
        "1.99.0",
        "test",
        "test",
        1,
        12,
        1,
        1,
        1,
    )
    .unwrap();
    let db = Database::open(&context, 99, 100, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 12);
    let original = db
        .hub_receipt_operation(owner(), operation(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.original.trace.origin, PopulationOrigin::Receipt);
    assert_eq!(original.original.trace.original_packs.value(), 20);
    assert_eq!(original.original.put_away.remaining().value(), 20);
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].position.packs.value(), 20);
    let catalog = db.hub_references(owner()).await.unwrap();
    assert_eq!(
        db.apply_hub_receipt(owner(), operation(1), command(&catalog), now())
            .await
            .unwrap(),
        original
    );
    db.close().await.unwrap();
    let db = Database::open(&context, 99, 101, &build).await.unwrap();
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap()
            .unwrap(),
        original
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(original_rows(&mut raw, 11).await, before);
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
    raw.close().await.unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&archive).unwrap())),
        ORIGINAL_SHA256
    );
}

#[tokio::test]
#[ignore = "Consumes the owned actual schema12 witness without recreating or resetting old data"]
async fn retained_schema12_opening_upgrade_preserves_originals_and_admits_distinct_counted_stock() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 13);
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let output = project
        .join("retained_evidence/harvestcircle-schema12-before-opening-v1")
        .canonicalize()
        .unwrap();
    assert_eq!(output.parent().unwrap().parent(), Some(project.as_path()));
    let witness: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("witness.json")).unwrap()).unwrap();
    assert_eq!(witness["schema"], 12);
    const SHA256: &str = "55f245830a385a4ee3e813efa4f591897c6648edbd6d2da1dc01341e36aa4d6d";
    let archive = PathBuf::from(witness["original_archive"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let profile = PathBuf::from(witness["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let state = PathBuf::from(witness["state_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(
        archive.starts_with(&output)
            && profile.starts_with(project.join("retained_evidence"))
            && state.starts_with(&profile)
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&archive).unwrap())),
        SHA256
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&state).unwrap())),
        SHA256,
        "consume only the original closed native schema12 world"
    );
    let options = SqliteConnectOptions::new()
        .filename(&state)
        .create_if_missing(false)
        .read_only(true);
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    let before = original_rows(&mut raw, 12).await;
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
        "3333333333333333333333333333333333333333",
        "189c49b74b4bafc142b00b76b296477931139e72",
        "1.99.0",
        "test",
        "test",
        1,
        13,
        1,
        1,
        1,
    )
    .unwrap();
    let db = Database::open(&context, 99, 100, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 13);
    let original = db
        .hub_receipt_operation(owner(), operation(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.original.trace.original_packs.value(), 20);
    assert_eq!(original.original.put_away.remaining().value(), 20);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(original_rows(&mut raw, 12).await, before);
    raw.close().await.unwrap();
    let db = Database::open(&context, 99, 101, &build).await.unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    assert_eq!(
        db.apply_hub_receipt(owner(), operation(1), command(&catalog), now())
            .await
            .unwrap(),
        original
    );
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let mut input = super::opening::tests::command(&catalog);
    input.expected_stock_revision = page.revision;
    let opening = db
        .apply_hub_opening(owner(), operation(2), input, now())
        .await
        .unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.position.packs.value())
            .sum::<u32>(),
        28
    );
    let counted = page
        .rows
        .iter()
        .find(|row| row.trace.id == opening.original.population.id)
        .unwrap();
    assert!(counted.put_away.is_none());
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original.clone())
    );
    db.close().await.unwrap();
    let db = Database::open(&context, 99, 102, &build).await.unwrap();
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(
        db.hub_opening_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(opening)
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
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
    raw.close().await.unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&archive).unwrap())),
        SHA256
    );
}

#[tokio::test]
#[ignore = "Consumes the owned actual schema13 witness without recreating or resetting old data"]
async fn retained_schema13_count_upgrade_preserves_originals_and_records_without_adjusting_stock() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 14);
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let output = project
        .join("retained_evidence/harvestcircle-schema13-before-counts-v1")
        .canonicalize()
        .unwrap();
    assert_eq!(output.parent().unwrap().parent(), Some(project.as_path()));
    let witness: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("witness.json")).unwrap()).unwrap();
    assert_eq!(witness["schema"], 13);
    const SHA256: &str = "6d4fae314ad860dc0e8ef0e5b52a735a455d137b5a2dfb17cdfb43d9ae03c2b7";
    let archive = PathBuf::from(witness["original_archive"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let profile = PathBuf::from(witness["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let state = PathBuf::from(witness["state_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(
        archive.starts_with(&output)
            && profile.starts_with(project.join("retained_evidence"))
            && state.starts_with(&profile)
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&archive).unwrap())),
        SHA256
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&state).unwrap())),
        SHA256,
        "consume only the original closed native schema13 world"
    );
    let options = SqliteConnectOptions::new()
        .filename(&state)
        .create_if_missing(false)
        .read_only(true);
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    let before = original_rows(&mut raw, 13).await;
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
        "3333333333333333333333333333333333333333",
        "189c49b74b4bafc142b00b76b296477931139e72",
        "1.99.0",
        "test",
        "test",
        1,
        14,
        1,
        1,
        1,
    )
    .unwrap();
    let db = Database::open(&context, 99, 100, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 14);
    let original = db
        .hub_receipt_operation(owner(), operation(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.original.trace.original_packs.value(), 20);
    assert_eq!(original.original.put_away.remaining().value(), 20);
    let opening = db
        .hub_opening_operation(owner(), operation(2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(opening.original.population.original_packs.value(), 8);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(original_rows(&mut raw, 13).await, before);
    raw.close().await.unwrap();
    let db = Database::open(&context, 99, 101, &build).await.unwrap();
    let catalog = db.hub_references(owner()).await.unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.rows.len(), 2);
    let row = page
        .rows
        .iter()
        .find(|row| row.trace.id == original.original.trace.id)
        .unwrap();
    let count = db
        .observe_hub_count(
            owner(),
            operation(3),
            harvestcircle_domain::hub::counts::CountObserveCommand {
                expected_stock_revision: page.revision,
                expected_catalog_revision: catalog.revision(),
                group: row.position.group,
                expected_group_revision: row.position.revision,
                counted_packs: harvestcircle_domain::hub::reference::WholePacks::from_value(18)
                    .unwrap(),
                cutoff: harvestcircle_domain::hub::stock::Occurrence::Known(now()),
                observation: harvestcircle_domain::hub::reference::SourceProvenance::new(
                    "Observed eighteen; original twenty remain recorded pending explicit review",
                )
                .unwrap(),
            },
            now(),
        )
        .await
        .unwrap();
    assert_eq!(count.original.scope.recorded_packs.value(), 20);
    assert_eq!(count.original.counted_packs.value(), 18);
    let after = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(after.rows, page.rows);
    assert_eq!(after.revision.value(), page.revision.value() + 1);
    db.close().await.unwrap();
    let db = Database::open(&context, 99, 102, &build).await.unwrap();
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(count)
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(
        db.hub_opening_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(opening)
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), after);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
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
    raw.close().await.unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&archive).unwrap())),
        SHA256
    );
}

#[tokio::test]
#[ignore = "Consumes the owned actual schema14 witness without recreating or resetting old data"]
async fn retained_schema14_reconciliation_upgrade_preserves_every_original_and_saved_count() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 15);
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let output = project
        .join("retained_evidence/harvestcircle-schema14-before-count-post-v1")
        .canonicalize()
        .unwrap();
    assert_eq!(output.parent().unwrap().parent(), Some(project.as_path()));
    let witness: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("witness.json")).unwrap()).unwrap();
    assert_eq!(witness["schema"], 14);
    const SHA256: &str = "ee2df44622c6cf1bcd12f844ab8c964a07f75dc6adfd6b735c8e9ab21278a3ce";
    let archive = PathBuf::from(witness["original_archive"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let profile = PathBuf::from(witness["profile_source_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let state = PathBuf::from(witness["state_file"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert!(
        archive.starts_with(&output)
            && profile.starts_with(project.join("retained_evidence"))
            && state.starts_with(&profile)
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&archive).unwrap())),
        SHA256
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&state).unwrap())),
        SHA256,
        "consume only the original closed native schema14 world"
    );
    let options = SqliteConnectOptions::new()
        .filename(&state)
        .create_if_missing(false)
        .read_only(true);
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    let before = original_rows(&mut raw, 14).await;
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
        "3333333333333333333333333333333333333333",
        "189c49b74b4bafc142b00b76b296477931139e72",
        "1.99.0",
        "test",
        "test",
        1,
        15,
        1,
        1,
        1,
    )
    .unwrap();
    let db = Database::open(&context, 99, 100, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 15);
    let original = db
        .hub_receipt_operation(owner(), operation(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.original.trace.original_packs.value(), 20);
    assert_eq!(original.original.put_away.remaining().value(), 20);
    let opening = db
        .hub_opening_operation(owner(), operation(2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(opening.original.population.original_packs.value(), 8);
    let count = db
        .hub_count_observation_operation(owner(), operation(3))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(count.original.scope.recorded_packs.value(), 20);
    assert_eq!(count.original.counted_packs.value(), 18);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(original_rows(&mut raw, 14).await, before);
    raw.close().await.unwrap();
    let db = Database::open(&context, 99, 101, &build).await.unwrap();
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    let scope = &count.original.scope;
    let input = harvestcircle_domain::hub::counts::CountObserveCommand {
        expected_stock_revision: scope.stock_revision,
        expected_catalog_revision: scope.catalog_revision,
        group: scope.group,
        expected_group_revision: scope.group_revision,
        counted_packs: count.original.counted_packs,
        cutoff: count.original.cutoff,
        observation: count.original.observation.clone(),
    };
    assert_eq!(
        db.observe_hub_count(owner(), operation(3), input, now())
            .await
            .unwrap(),
        count
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    assert_eq!(page.revision.value(), 4);
    assert_eq!(
        page.rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        28
    );
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    for sql in [
        "SELECT count(*) FROM hub_count_reconciliations",
        "SELECT count(*) FROM hub_count_reconciliation_work",
        "SELECT count(*) FROM hub_counted_surplus",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(&mut raw)
                .await
                .unwrap(),
            0
        );
    }
    raw.close().await.unwrap();
    db.close().await.unwrap();
    let db = Database::open(&context, 99, 102, &build).await.unwrap();
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(count)
    );
    assert_eq!(
        db.hub_receipt_operation(owner(), operation(1))
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(
        db.hub_opening_operation(owner(), operation(2))
            .await
            .unwrap(),
        Some(opening)
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
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
    raw.close().await.unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&archive).unwrap())),
        SHA256
    );
}
