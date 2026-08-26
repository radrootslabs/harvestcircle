use core::num::NonZeroU32;
use std::fs;
use std::path::Path;

use harvestcircle_domain::{SafeError, SafeErrorCode, SafeMessage};
use radroots_runtime_paths::RuntimeContext;
use radroots_service_sqlite::{
    ExistingServiceDatabaseIntent, MigrationAppliedAtUnixSeconds, MigrationBuildIdentity, OpenMode,
    ServiceDatabaseMetadata, ServiceSqliteConnectionOptions, ServiceSqliteErrorKind,
    ServiceSqliteHost, ServiceSqliteTransactionError, ServiceSqliteTransactionErrorKind,
    initialize_database,
};
use radroots_storage::event::SourceGeneration;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};

use crate::contract::harvestcircle_initial_schema_sql;
use crate::{HARVESTCIRCLE_STATE_SCHEMA_VERSION, HarvestCircleStorageContract};

pub const CURRENT_SCHEMA_VERSION: u32 = HARVESTCIRCLE_STATE_SCHEMA_VERSION;

/// The sole HarvestCircle SQLite host.
///
/// The underlying pool and connections remain sealed by `radroots_service_sqlite`.
pub struct Database {
    host: ServiceSqliteHost,
    metadata: ServiceDatabaseMetadata,
}

impl Database {
    /// Opens or initializes the canonical HarvestCircle service-instance state.
    ///
    /// The caller injects creation and migration evidence. Fresh state receives
    /// a new opaque source generation from host entropy; existing state is
    /// admitted through its persisted identity without guessing that generation.
    pub async fn open(
        context: &RuntimeContext,
        created_at_unix_ms: u64,
        applied_at_unix_s: u64,
        build: &MigrationBuildIdentity,
    ) -> Result<Self, SafeError> {
        let contract = HarvestCircleStorageContract::from_runtime_context(context)
            .map_err(|_| invalid_storage_contract())?;
        provision_state_directory(contract.paths().state_database())?;
        let applied_at = MigrationAppliedAtUnixSeconds::new(applied_at_unix_s)
            .map_err(|_| invalid_storage_contract())?;
        let options = ServiceSqliteConnectionOptions::reviewed();

        if contract
            .paths()
            .state_database()
            .try_exists()
            .map_err(|_| storage_unavailable())?
        {
            return Self::open_existing(&contract, applied_at, build).await;
        }

        let mut generation = [0_u8; 32];
        getrandom::getrandom(&mut generation).map_err(|_| storage_unavailable())?;
        let generation = SourceGeneration::new(generation).map_err(|_| storage_unavailable())?;
        let metadata = ServiceDatabaseMetadata::new(
            contract.paths(),
            generation,
            NonZeroU32::new(CURRENT_SCHEMA_VERSION).expect("schema v1 is nonzero"),
            created_at_unix_ms,
            contract.application_id(),
        )
        .map_err(|_| invalid_storage_contract())?;
        let authority = initialize_database(
            contract.paths(),
            OpenMode::Initialize,
            &metadata,
            contract.schema(),
            initialize_application_schema,
        )
        .await
        .map_err(map_service_error)?;
        let (host, _) = ServiceSqliteHost::open_initialized(
            contract.paths(),
            &metadata.identity(),
            contract.migrations(),
            contract.schema(),
            options,
            authority,
            applied_at,
            build,
            &[],
        )
        .await
        .map_err(map_service_error)?;
        Ok(Self { host, metadata })
    }

    /// Explicitly drains SQLite work, checkpoints WAL state, and releases authority.
    pub async fn close(&self) -> Result<(), SafeError> {
        self.host.close().await.map_err(map_service_error)
    }

    #[must_use]
    pub const fn metadata(&self) -> &ServiceDatabaseMetadata {
        &self.metadata
    }

    pub(crate) const fn host(&self) -> &ServiceSqliteHost {
        &self.host
    }

    pub(crate) async fn open_existing(
        contract: &HarvestCircleStorageContract,
        applied_at: MigrationAppliedAtUnixSeconds,
        build: &MigrationBuildIdentity,
    ) -> Result<Self, SafeError> {
        let intent = ExistingServiceDatabaseIntent::new(
            contract.paths(),
            contract.state_schema_version(),
            contract.application_id(),
        );
        let (opened, _) = ServiceSqliteHost::open_read_write_existing_with_intent(
            contract.paths(),
            &intent,
            contract.migrations(),
            contract.schema(),
            ServiceSqliteConnectionOptions::reviewed(),
            applied_at,
            build,
            &[],
        )
        .await
        .map_err(map_service_error)?;
        let (host, metadata) = opened.into_parts();
        Ok(Self { host, metadata })
    }
}

async fn initialize_application_schema(path: std::path::PathBuf) -> Result<(), sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    for statement in harvestcircle_initial_schema_sql() {
        sqlx::query(*statement).execute(&mut connection).await?;
    }
    sqlx::query("INSERT INTO runtime_state (singleton) VALUES (1)")
        .execute(&mut connection)
        .await?;
    connection.close().await
}

fn provision_state_directory(database: &Path) -> Result<(), SafeError> {
    let directory = database.parent().ok_or_else(invalid_storage_contract)?;
    fs::create_dir_all(directory).map_err(|_| storage_unavailable())?;
    let metadata = fs::symlink_metadata(directory).map_err(|_| storage_unavailable())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(storage_unavailable());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| storage_unavailable())?;
    }
    Ok(())
}

pub(crate) fn map_transaction_error(error: ServiceSqliteTransactionError<SafeError>) -> SafeError {
    if let Some(operation) = error.operation_error() {
        return *operation;
    }
    match error.kind() {
        ServiceSqliteTransactionErrorKind::CommitOutcomeUnknown => commit_outcome_unknown(),
        ServiceSqliteTransactionErrorKind::NotCommitted
        | ServiceSqliteTransactionErrorKind::OperationRolledBack
        | ServiceSqliteTransactionErrorKind::RollbackFailed => storage_unavailable(),
    }
}

pub(crate) fn map_service_error(error: radroots_service_sqlite::ServiceSqliteError) -> SafeError {
    match error.kind() {
        ServiceSqliteErrorKind::Metadata
        | ServiceSqliteErrorKind::Migration
        | ServiceSqliteErrorKind::Integrity
        | ServiceSqliteErrorKind::Recovery => corrupt_storage(),
        ServiceSqliteErrorKind::Authority
        | ServiceSqliteErrorKind::Open
        | ServiceSqliteErrorKind::Create
        | ServiceSqliteErrorKind::Pragma
        | ServiceSqliteErrorKind::Backup
        | ServiceSqliteErrorKind::Restore => storage_unavailable(),
    }
}

pub(crate) const fn storage_unavailable() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The application state is unavailable."),
    )
}

pub(crate) const fn corrupt_storage() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageCorrupt,
        SafeMessage::new("The application state could not be verified."),
    )
}

pub(crate) const fn invalid_storage_contract() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The application storage contract is invalid."),
    )
}

const fn commit_outcome_unknown() -> SafeError {
    SafeError::new(
        SafeErrorCode::PendingOperationRecoveryRequired,
        SafeMessage::new("The storage commit outcome must be reconciled."),
    )
}
