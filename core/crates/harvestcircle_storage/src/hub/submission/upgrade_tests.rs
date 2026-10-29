//! Consumes the retained, closed schema15 world once; never fabricates an old database.
use super::*;
use crate::hub::stock::{
    tests::{command, now, operation, owner, query},
    upgrade_tests::original_rows,
};
use harvestcircle_application::SessionGeneration;
use harvestcircle_domain::{
    SafeErrorCode,
    hub::{counts::CountObserveCommand, reference::SourceProvenance},
};
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::PathBuf;

#[tokio::test]
#[ignore = "Consumes the owned actual schema15 witness once; never resets or creates an old world"]
async fn retained_schema15_admission_upgrade_preserves_originals_and_pending_semantics() {
    assert_eq!(crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION, 16);
    let project = PathBuf::from(std::env::var_os("EXT_BUILD_PROJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let output = project
        .join("retained_evidence/harvestcircle-schema15-before-admission-v1")
        .canonicalize()
        .unwrap();
    assert_eq!(output.parent().unwrap().parent(), Some(project.as_path()));
    let witness: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("witness.json")).unwrap()).unwrap();
    assert_eq!(witness["schema"], 15);
    const SHA256: &str = "5dc7c764c39543e816cb4c02da53e1547c8b0210ddc1f5fdfbecbe589693ebe2";
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
        "consume only the original closed schema15 world"
    );
    let options = SqliteConnectOptions::new()
        .filename(&state)
        .create_if_missing(false)
        .read_only(true);
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    let before = original_rows(&mut raw, 15).await;
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
        "4444444444444444444444444444444444444444",
        "189c49b74b4bafc142b00b76b296477931139e72",
        "1.99.0",
        "test",
        "test",
        1,
        16,
        1,
        1,
        1,
    )
    .unwrap();
    let db = Database::open(&context, 99, 100, &build).await.unwrap();
    assert_eq!(db.metadata().state_schema_version().get(), 16);
    let original = db
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
    assert_eq!(original.original.trace.original_packs.value(), 20);
    assert_eq!(original.original.put_away.remaining().value(), 20);
    assert_eq!(opening.original.population.original_packs.value(), 8);
    assert_eq!(count.original.scope.recorded_packs.value(), 20);
    assert_eq!(count.original.counted_packs.value(), 18);
    let page = db.hub_stock_page(owner(), query()).await.unwrap();
    assert_eq!(page.revision.value(), 4);
    assert_eq!(
        page.rows
            .iter()
            .map(|r| r.position.packs.value())
            .sum::<u32>(),
        28
    );
    let catalog = db.hub_references(owner()).await.unwrap();
    let runtime = db.begin_hub_runtime(now()).await.unwrap();
    let generation = db
        .advance_hub_session(runtime, SessionGeneration::initial(), Some(owner()), now())
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime,
        generation,
        actor: owner(),
    };
    let replay = db
        .submit_hub_mutation(
            scope,
            operation(1),
            HubMutation::Receive(Box::new(command(&catalog))),
            now(),
        )
        .await
        .unwrap();
    let HubMutationResult::Received(replay) = replay else {
        panic!("original receipt")
    };
    assert_eq!(*replay, original);
    let pending = CountObserveCommand {
        expected_stock_revision: page.revision,
        expected_catalog_revision: catalog.revision(),
        group: count.original.scope.group,
        expected_group_revision: count.original.scope.group_revision,
        counted_packs: count.original.counted_packs,
        cutoff: count.original.cutoff,
        observation: SourceProvenance::new(
            "Retained fictional count intent deliberately interrupted before effects",
        )
        .unwrap(),
    };
    let owned = pending.clone();
    let grant = db
        .host()
        .transaction(|tx| {
            Box::pin(async move {
                admission::prepare_on(
                    tx,
                    scope,
                    operation(4),
                    HubMutation::CountObservation(Box::new(owned)),
                    now(),
                )
                .await
            })
        })
        .await
        .unwrap();
    assert!(matches!(grant, admission::Decision::Fresh(_)));
    drop(grant);
    let saved = db
        .hub_submission(owner(), operation(4))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, HubSubmissionStatus::Pending);
    assert_eq!(saved.scope, scope);
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(original_rows(&mut raw, 15).await, before);
    raw.close().await.unwrap();
    let db = Database::open(&context, 99, 101, &build).await.unwrap();
    let next_runtime = db.begin_hub_runtime(now()).await.unwrap();
    assert_ne!(next_runtime, runtime);
    assert_eq!(db.hub_runtime_session().await.unwrap().unwrap().actor, None);
    let next = db
        .advance_hub_session(
            next_runtime,
            SessionGeneration::initial(),
            Some(owner()),
            now(),
        )
        .await
        .unwrap();
    let scope = HubSubmissionScope {
        runtime: next_runtime,
        generation: next,
        actor: owner(),
    };
    assert_eq!(
        db.submit_hub_mutation(
            scope,
            operation(4),
            HubMutation::CountObservation(Box::new(pending)),
            now()
        )
        .await
        .unwrap_err()
        .code(),
        SafeErrorCode::HubOutcomeUnknown
    );
    assert_eq!(
        db.hub_submission(owner(), operation(4))
            .await
            .unwrap()
            .unwrap(),
        saved
    );
    assert_eq!(db.hub_stock_page(owner(), query()).await.unwrap(), page);
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
    assert_eq!(
        db.hub_count_observation_operation(owner(), operation(3))
            .await
            .unwrap(),
        Some(count)
    );
    db.close().await.unwrap();
    let mut raw = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(original_rows(&mut raw, 15).await, before);
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
