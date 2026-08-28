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

fn unresolved_runtime_context(directory: &TempDir) -> RuntimeContext {
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

fn runtime_context(directory: &TempDir) -> RuntimeContext {
    let context = unresolved_runtime_context(directory);
    fs::create_dir_all(directory.path().join("data")).expect("state root");
    context
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
async fn bootstrap_requires_the_existing_governed_state_root() {
    let directory = tempdir().expect("directory");
    let context = unresolved_runtime_context(&directory);
    let state_root = directory.path().join("data");
    let error = Database::open(&context, 1, 1, &build_identity())
        .await
        .err()
        .expect("missing state root must fail closed");

    assert_eq!(error.code(), SafeErrorCode::StorageUnavailable);
    assert!(!state_root.exists());
    assert!(!context.paths().state().exists());
}

#[test]
fn bootstrap_source_uses_only_the_governed_path_and_sqlite_boundaries() {
    const SOURCE: &str = include_str!("../src/db.rs");

    for required in [
        ".state_directory_plan()",
        "ServiceSqliteHost::open_or_initialize",
        "ServiceSqliteInitializer",
        "opened.into_parts()",
    ] {
        assert!(SOURCE.contains(required), "missing `{required}`");
    }
    for forbidden in [
        ".try_exists()",
        "create_dir_all",
        "set_permissions",
        "SqliteConnectOptions",
        "sqlx::SqliteConnection",
        "initialize_database",
        "open_initialized",
    ] {
        assert!(!SOURCE.contains(forbidden), "forbidden `{forbidden}`");
    }
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
    assert_eq!(database.metadata().state_schema_version().get(), 2);

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
    assert_eq!(reopened.metadata().state_schema_version().get(), 2);
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

    let receipt = database
        .finalize_durable_operation(
            &requests[0],
            DurableOperationPhase::IntentRecorded,
            DurableTerminalOutcome::Completed,
            None,
            UnixTimestamp::from_seconds(3).expect("time"),
        )
        .await
        .expect("finalize one operation");
    assert_eq!(receipt.completed_at().as_seconds(), 3);
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

async fn seed_terminal_operations(
    connection: &mut SqliteConnection,
    count: usize,
    completed_at: impl Fn(usize) -> i64,
) {
    let identity = PublicKey::from_bytes([7; 32]).expect("public key");
    let mut transaction = connection.begin().await.expect("fixture transaction");
    for index in 0..count {
        let request = DurableRequestId::new_v7();
        let completed_at = completed_at(index);
        sqlx::query(
            "INSERT INTO durable_operations (request_id, operation_kind, account_public_key, \
             binding_public_key, phase, terminal_outcome, updated_at_unix_s, \
             completed_at_unix_s) \
             VALUES (?, 'import', ?, ?, 'finalized', 'completed', ?, ?)",
        )
        .bind(request.as_str())
        .bind(identity.as_bytes().as_slice())
        .bind(identity.as_bytes().as_slice())
        .bind(completed_at)
        .bind(completed_at)
        .execute(&mut *transaction)
        .await
        .expect("terminal fixture operation");
    }
    transaction.commit().await.expect("fixture commit");
}

#[tokio::test]
async fn terminal_retention_is_bounded_batched_and_never_evicts_in_window_receipts() {
    const TOTAL_CAPACITY: usize = 4_096;
    const CLEANUP_BATCH: usize = 256;
    const RETENTION_SECONDS: i64 = 7 * 24 * 60 * 60;
    const EXPIRED: usize = 300;
    const NOW: i64 = 1_000_000;

    let directory = tempdir().expect("directory");
    let context = runtime_context(&directory);
    let database_path = context.paths().state().join("state.sqlite");
    let build = build_identity();
    let database = Database::open(&context, 1, 1, &build)
        .await
        .expect("database");
    database.close().await.expect("fixture close");

    let options = SqliteConnectOptions::new()
        .filename(&database_path)
        .create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("fixture connection");
    seed_terminal_operations(&mut connection, TOTAL_CAPACITY, |index| {
        if index < EXPIRED {
            NOW - RETENTION_SECONDS - 1
        } else {
            NOW - RETENTION_SECONDS
        }
    })
    .await;
    connection.close().await.expect("fixture close");

    let database = Database::open(&context, 2, 2, &build)
        .await
        .expect("reopen");
    let identity = PublicKey::from_bytes([7; 32]).expect("public key");
    database
        .begin_durable_operation(
            &DurableRequestId::new_v7(),
            DurableOperationKind::Import,
            identity,
            None,
            OperationPriorState::new(None, None),
            UnixTimestamp::from_seconds(NOW).expect("time"),
        )
        .await
        .expect("admission after bounded cleanup");
    database.close().await.expect("close after cleanup");

    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("inspect connection");
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM durable_operations")
        .fetch_one(&mut connection)
        .await
        .expect("total");
    let expired: i64 =
        sqlx::query_scalar("SELECT count(*) FROM durable_operations WHERE completed_at_unix_s < ?")
            .bind(NOW - RETENTION_SECONDS)
            .fetch_one(&mut connection)
            .await
            .expect("expired");
    let in_window: i64 =
        sqlx::query_scalar("SELECT count(*) FROM durable_operations WHERE completed_at_unix_s = ?")
            .bind(NOW - RETENTION_SECONDS)
            .fetch_one(&mut connection)
            .await
            .expect("in-window");
    assert_eq!(
        total,
        i64::try_from(TOTAL_CAPACITY - CLEANUP_BATCH + 1).unwrap()
    );
    assert_eq!(expired, i64::try_from(EXPIRED - CLEANUP_BATCH).unwrap());
    assert_eq!(in_window, i64::try_from(TOTAL_CAPACITY - EXPIRED).unwrap());
    connection.close().await.expect("inspection close");

    let database = Database::open(&context, 3, 3, &build)
        .await
        .expect("second reopen");
    database
        .begin_durable_operation(
            &DurableRequestId::new_v7(),
            DurableOperationKind::Import,
            identity,
            None,
            OperationPriorState::new(None, None),
            UnixTimestamp::from_seconds(NOW).expect("time"),
        )
        .await
        .expect("admission after remaining cleanup");
    database.close().await.expect("second cleanup close");

    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("final inspection");
    let expired: i64 =
        sqlx::query_scalar("SELECT count(*) FROM durable_operations WHERE completed_at_unix_s < ?")
            .bind(NOW - RETENTION_SECONDS)
            .fetch_one(&mut connection)
            .await
            .expect("remaining expired");
    let in_window: i64 =
        sqlx::query_scalar("SELECT count(*) FROM durable_operations WHERE completed_at_unix_s = ?")
            .bind(NOW - RETENTION_SECONDS)
            .fetch_one(&mut connection)
            .await
            .expect("remaining in-window");
    assert_eq!(expired, 0);
    assert_eq!(in_window, i64::try_from(TOTAL_CAPACITY - EXPIRED).unwrap());
    connection.close().await.expect("final inspection close");
}

#[tokio::test]
async fn total_capacity_reserves_terminal_receipts_without_in_window_eviction() {
    const TOTAL_CAPACITY: usize = 4_096;
    const NOW: i64 = 1_000_000;

    let directory = tempdir().expect("directory");
    let context = runtime_context(&directory);
    let database_path = context.paths().state().join("state.sqlite");
    let build = build_identity();
    let database = Database::open(&context, 1, 1, &build)
        .await
        .expect("database");
    database.close().await.expect("fixture close");

    let options = SqliteConnectOptions::new()
        .filename(&database_path)
        .create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("fixture connection");
    seed_terminal_operations(&mut connection, TOTAL_CAPACITY - 1, |_| NOW).await;
    connection.close().await.expect("fixture close");

    let database = Database::open(&context, 2, 2, &build)
        .await
        .expect("reopen");
    let reserved = DurableRequestId::new_v7();
    database
        .begin_durable_operation(
            &reserved,
            DurableOperationKind::Create,
            PublicKey::from_bytes([7; 32]).expect("public key"),
            None,
            OperationPriorState::new(None, None),
            UnixTimestamp::from_seconds(NOW).expect("time"),
        )
        .await
        .expect("last row must reserve its terminal receipt");
    database
        .finalize_durable_operation(
            &reserved,
            DurableOperationPhase::IntentRecorded,
            DurableTerminalOutcome::Completed,
            None,
            UnixTimestamp::from_seconds(NOW + 1).expect("time"),
        )
        .await
        .expect("reserved row must finalize in place");
    let error = database
        .begin_durable_operation(
            &DurableRequestId::new_v7(),
            DurableOperationKind::Create,
            PublicKey::from_bytes([7; 32]).expect("public key"),
            None,
            OperationPriorState::new(None, None),
            UnixTimestamp::from_seconds(NOW).expect("time"),
        )
        .await
        .expect_err("total capacity must reserve the terminal journal");
    assert_eq!(error.code(), SafeErrorCode::InvalidApplicationState);
    database.close().await.expect("close");

    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("inspection connection");
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM durable_operations")
        .fetch_one(&mut connection)
        .await
        .expect("total");
    assert_eq!(total, i64::try_from(TOTAL_CAPACITY).unwrap());
    connection.close().await.expect("inspection close");
}
