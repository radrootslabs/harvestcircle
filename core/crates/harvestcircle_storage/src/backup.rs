use core::{fmt, num::NonZeroU64};
use std::path::Path;

use radroots_runtime_paths::RuntimeContext;
use radroots_service_sqlite::{
    BackupCreatedAtUnixMs, BackupManifestSha256, MigrationAppliedAtUnixSeconds,
    MigrationBuildIdentity, ServiceBackupManifest, ServiceDatabaseIdentity, VerifiedServiceBackup,
    finalize_staged_restore, stage_verified_restore, verify_backup_bundle,
};

use crate::db::{invalid_storage_contract, map_service_error};
use crate::{Database, HarvestCircleStorageContract};
use harvestcircle_domain::{SafeError, SafeErrorCode, SafeMessage};

/// Retained, non-forgeable proof of one identity-bound HarvestCircle backup.
///
/// This type intentionally exposes no path, descriptor, or raw database handle.
/// It is single-use restore authority and cannot be cloned:
///
/// ```compile_fail
/// use harvestcircle_storage::VerifiedHarvestCircleBackup;
///
/// fn require_clone<T: Clone>() {}
/// require_clone::<VerifiedHarvestCircleBackup>();
/// ```
pub struct VerifiedHarvestCircleBackup {
    inner: VerifiedServiceBackup,
}

impl fmt::Debug for VerifiedHarvestCircleBackup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedHarvestCircleBackup([redacted])")
    }
}

/// Verifies one untrusted canonical backup bundle without mutating it.
///
/// The expected digest, database identity, and positive member limit are
/// trusted inputs. This operation is synchronous and performs bounded file and
/// SQLite reads; callers own any supervised worker and deadline.
pub fn verify_harvestcircle_backup(
    manifest_bytes: &[u8],
    expected_manifest_digest: BackupManifestSha256,
    bundle_directory: &Path,
    expected_identity: &ServiceDatabaseIdentity,
    maximum_state_bytes: NonZeroU64,
) -> Result<VerifiedHarvestCircleBackup, SafeError> {
    verify_backup_bundle(
        manifest_bytes,
        expected_manifest_digest,
        bundle_directory,
        expected_identity,
        maximum_state_bytes,
    )
    .map(|inner| VerifiedHarvestCircleBackup { inner })
    .map_err(|_| invalid_backup())
}

impl Database {
    /// Captures one point-in-time backup into a caller-selected new directory.
    pub async fn capture_online_backup(
        &self,
        staging_directory: &Path,
        created_at_unix_ms: BackupCreatedAtUnixMs,
    ) -> Result<ServiceBackupManifest, SafeError> {
        self.host()
            .capture_online_backup(staging_directory, created_at_unix_ms)
            .await
            .map_err(map_service_error)
    }

    /// Closes live state, installs one verified backup, and reopens recovered state.
    ///
    /// Exclusive mutable access prevents concurrent use of the pre-restore
    /// host. Preflight failure leaves that host open and usable. Failure after
    /// close may leave exact recovery evidence; a later ordinary
    /// `Database::open` reconciles that evidence.
    pub async fn restore_verified_backup(
        &mut self,
        context: &RuntimeContext,
        verified: VerifiedHarvestCircleBackup,
        applied_at_unix_s: u64,
        build: &MigrationBuildIdentity,
    ) -> Result<(), SafeError> {
        let contract = HarvestCircleStorageContract::from_runtime_context(context)
            .map_err(|_| invalid_storage_contract())?;
        let expected = self.metadata().identity();
        let backup_metadata = verified.inner.database_metadata();
        if expected.service() != contract.paths().service()
            || expected.instance() != contract.paths().instance()
            || expected.supported_state_schema_version() != contract.state_schema_version()
            || expected.application_id() != contract.application_id()
            || backup_metadata.service() != expected.service()
            || backup_metadata.instance() != expected.instance()
            || backup_metadata.source_generation() != expected.source_generation()
            || backup_metadata.application_id() != expected.application_id()
            || backup_metadata.state_schema_version() > expected.supported_state_schema_version()
        {
            return Err(invalid_storage_contract());
        }
        let applied_at = MigrationAppliedAtUnixSeconds::new(applied_at_unix_s)
            .map_err(|_| invalid_storage_contract())?;

        self.close().await?;
        let staged = stage_verified_restore(
            contract.paths(),
            &expected,
            contract.migrations(),
            contract.schema(),
            verified.inner,
        )
        .await
        .map_err(map_restore_error)?;
        finalize_staged_restore(staged)
            .await
            .map_err(map_restore_error)?;
        let reopened = Self::open_existing(&contract, applied_at, build).await?;
        *self = reopened;
        Ok(())
    }
}

const fn invalid_backup() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageBackupInvalid,
        SafeMessage::new("The selected backup could not be verified."),
    )
}

fn map_restore_error(error: radroots_service_sqlite::ServiceSqliteError) -> SafeError {
    use radroots_service_sqlite::ServiceSqliteErrorKind;

    match error.kind() {
        ServiceSqliteErrorKind::Metadata
        | ServiceSqliteErrorKind::Migration
        | ServiceSqliteErrorKind::Integrity
        | ServiceSqliteErrorKind::Backup => invalid_backup(),
        ServiceSqliteErrorKind::Recovery => SafeError::new(
            SafeErrorCode::StorageQuarantined,
            SafeMessage::new("The application state requires recovery."),
        ),
        ServiceSqliteErrorKind::Authority
        | ServiceSqliteErrorKind::Open
        | ServiceSqliteErrorKind::Create
        | ServiceSqliteErrorKind::Pragma
        | ServiceSqliteErrorKind::Restore => map_service_error(error),
    }
}
