use std::fs;

use harvestcircle_application::{
    DurableOperationKind, DurableOperationPhase, DurableOperationRepository, DurableOperationStart,
    DurableRequestId, DurableTerminalOutcome, IdentityRepository, KeyMaterialProvider,
    OperationPriorState,
};
use harvestcircle_domain::{
    IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, PublicKey,
    SafeErrorCode, SignerAvailability, UnixTimestamp,
};
use harvestcircle_nostr::NostrKeyMaterialProvider;
use harvestcircle_storage::{
    Database, HARVESTCIRCLE_IDENTITY_CAPACITY, HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY,
};
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};
use tempfile::{TempDir, tempdir_in};

fn tempdir() -> std::io::Result<TempDir> {
    tempdir_in(std::env::temp_dir().canonicalize()?)
}

fn runtime_context(directory: &TempDir) -> RuntimeContext {
    RuntimeContext::resolve(
        &RadrootsPathResolver::new(
            RadrootsPlatform::current(),
            RadrootsHostEnvironment::default(),
        ),
        RuntimeContextBootstrap::new(
            RadrootsPathProfile::RepoLocal,
            Some(
                directory
                    .path()
                    .canonicalize()
                    .expect("canonical directory"),
            ),
            RuntimeContextSource::BootstrapCli,
            RuntimeContextSource::SafeDefault,
        )
        .expect("bootstrap"),
        ServiceId::new("harvestcircle").expect("service"),
        InstanceId::new("desktop").expect("instance"),
    )
    .expect("runtime context")
}

fn build_identity() -> MigrationBuildIdentity {
    MigrationBuildIdentity::new(
        "0.1.0-alpha",
        "1111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222",
        "1.97.1",
        "test",
        "test",
        1,
        1,
        1,
        1,
        1,
    )
    .expect("build identity")
}

fn identity(index: usize) -> NostrIdentity {
    let (public_key, npub, secret, nsec) = NostrKeyMaterialProvider
        .generate()
        .expect("generated key material")
        .into_parts();
    drop((secret, nsec));
    NostrIdentity::new(
        NostrIdentityReference::verify(public_key, npub.as_str().to_owned())
            .expect("identity reference"),
        LocalKeyringBinding::new(public_key, SignerAvailability::Available),
        None,
        IdentityCreatedAt::new(UnixTimestamp::from_seconds(index as i64 + 1).expect("time")),
        None,
    )
    .expect("identity")
}

#[tokio::test]
async fn canonical_database_preserves_legacy_state_and_enforces_identity_capacity() {
    let directory = tempdir().expect("directory");
    let legacy = directory.path().join("harvestcircle.sqlite3");
    fs::write(&legacy, b"legacy-state-must-remain-untouched").expect("legacy sentinel");
    let context = runtime_context(&directory);
    let database_path = context.paths().state().join("state.sqlite");
    let build = build_identity();
    let database = Database::open(&context, 1, 1, &build)
        .await
        .expect("database");
    let generation = database.metadata().source_generation();

    let first_identity = identity(0);
    database
        .insert_identity(&first_identity)
        .await
        .expect("first identity");
    for index in 1..HARVESTCIRCLE_IDENTITY_CAPACITY {
        database
            .insert_identity(&identity(index))
            .await
            .expect("identity within capacity");
    }
    assert_eq!(
        database
            .list_identities()
            .await
            .expect("identity list")
            .len(),
        HARVESTCIRCLE_IDENTITY_CAPACITY
    );
    let duplicate = database
        .insert_identity(&first_identity)
        .await
        .expect_err("duplicate at capacity");
    assert_eq!(duplicate.code(), SafeErrorCode::IdentityAlreadyExists);
    let excess = database
        .insert_identity(&identity(HARVESTCIRCLE_IDENTITY_CAPACITY))
        .await
        .expect_err("capacity must reject");
    assert_eq!(excess.code(), SafeErrorCode::InvalidApplicationState);
    database.close().await.expect("close");

    assert_eq!(
        fs::read(&legacy).expect("legacy sentinel"),
        b"legacy-state-must-remain-untouched"
    );
    assert!(database_path.is_file());
    assert_ne!(database_path, legacy);

    let reopened = Database::open(&context, 2, 2, &build)
        .await
        .expect("reopen");
    assert_eq!(reopened.metadata().source_generation(), generation);
    assert_eq!(
        reopened
            .list_identities()
            .await
            .expect("reopened identities")
            .len(),
        HARVESTCIRCLE_IDENTITY_CAPACITY
    );
    reopened.close().await.expect("reopened close");
}

#[tokio::test]
async fn unfinished_uuid_ledger_enforces_exact_capacity_and_recovers_after_finalize() {
    let directory = tempdir().expect("directory");
    let context = runtime_context(&directory);
    let database_path = context.paths().state().join("state.sqlite");
    let build = build_identity();
    let database = Database::open(&context, 1, 1, &build)
        .await
        .expect("database");
    database.close().await.expect("close before fixture load");

    let options = SqliteConnectOptions::new()
        .filename(&database_path)
        .create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("fixture connection");
    let mut transaction = connection.begin().await.expect("fixture transaction");
    let identity = PublicKey::from_bytes([9; 32]).expect("public key");
    let mut requests = Vec::with_capacity(HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY);
    for _ in 0..HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY {
        let request = DurableRequestId::new_v7();
        sqlx::query(
            "INSERT INTO durable_operations (request_id, operation_kind, account_public_key, \
             binding_public_key, phase, updated_at_unix_s) \
             VALUES (?, 'import', ?, ?, 'intent_recorded', 1)",
        )
        .bind(request.as_str())
        .bind(identity.as_bytes().as_slice())
        .bind(identity.as_bytes().as_slice())
        .execute(&mut *transaction)
        .await
        .expect("fixture operation");
        requests.push(request);
    }
    transaction.commit().await.expect("fixture commit");
    connection.close().await.expect("fixture close");

    let database = Database::open(&context, 2, 2, &build)
        .await
        .expect("reopen");
    assert_eq!(
        database
            .list_unfinished_durable_operations()
            .await
            .expect("unfinished operations")
            .len(),
        HARVESTCIRCLE_UNFINISHED_DURABLE_OPERATION_CAPACITY
    );
    assert!(matches!(
        database
            .begin_durable_operation(
                &requests[0],
                DurableOperationKind::Import,
                identity,
                None,
                OperationPriorState::new(None, None),
                UnixTimestamp::from_seconds(2).expect("time"),
            )
            .await
            .expect("matching replay at capacity"),
        DurableOperationStart::Existing(_)
    ));
    let excess = DurableRequestId::new_v7();
    let error = database
        .begin_durable_operation(
            &excess,
            DurableOperationKind::Import,
            identity,
            None,
            OperationPriorState::new(None, None),
            UnixTimestamp::from_seconds(2).expect("time"),
        )
        .await
        .expect_err("capacity must reject");
    assert_eq!(error.code(), SafeErrorCode::InvalidApplicationState);

    database
        .finalize_durable_operation(
            &requests[0],
            DurableOperationPhase::IntentRecorded,
            DurableTerminalOutcome::Completed,
            None,
            UnixTimestamp::from_seconds(3).expect("time"),
        )
        .await
        .expect("finalize one operation");
    database
        .begin_durable_operation(
            &excess,
            DurableOperationKind::Import,
            identity,
            None,
            OperationPriorState::new(None, None),
            UnixTimestamp::from_seconds(4).expect("time"),
        )
        .await
        .expect("capacity recovered");
    database.close().await.expect("close");
}
