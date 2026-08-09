use std::fs::{self, File, OpenOptions};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use fs2::FileExt;
use radroots_studio_domain::{SafeError, SafeErrorCode, SafeMessage};
use refinery::embed_migrations;
use rusqlite::{Connection, OpenFlags};

use crate::compatibility::{DatabasePreflight, preflight, quarantined_storage_error};
use crate::recovery::MigrationRecovery;
use crate::repair::{
    QuarantineExportReceipt, RepairAuthorization, RepairCandidate, authenticate_candidate,
    export_quarantined, install_candidate,
};

pub const CURRENT_SCHEMA_VERSION: u32 = 10;

mod migrations {
    use super::embed_migrations;

    embed_migrations!("migrations");
}

pub struct Database {
    connection: Mutex<Connection>,
    path: Option<PathBuf>,
    _ownership: Option<WritableOwnership>,
}

pub(crate) struct DatabaseConnection<'a> {
    connection: MutexGuard<'a, Connection>,
    path: Option<&'a Path>,
}

struct WritableOwnership {
    _file: File,
}

impl Database {
    /// Opens, configures, and migrates a file-backed `SQLite` database.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the file, connection configuration,
    /// permission update, or migration cannot complete.
    pub fn open(path: &Path) -> Result<Self, SafeError> {
        let preflight = preflight(path)?;
        if matches!(&preflight, DatabasePreflight::Quarantined { .. }) {
            return Err(quarantined_storage_error());
        }
        let parent = path.parent().ok_or_else(storage_error)?;
        create_secure_directory(parent)?;
        restrict_sqlite_sidecars(path)?;
        let ownership = WritableOwnership::acquire(path)?;
        let recovery_source_schema = match &preflight {
            DatabasePreflight::Ready { schema_version }
                if *schema_version < CURRENT_SCHEMA_VERSION =>
            {
                Some(*schema_version)
            }
            _ => None,
        };
        let recovery = match preflight {
            DatabasePreflight::Ready { schema_version }
                if schema_version < CURRENT_SCHEMA_VERSION =>
            {
                Some(MigrationRecovery::prepare(
                    path,
                    schema_version,
                    CURRENT_SCHEMA_VERSION,
                )?)
            }
            DatabasePreflight::Fresh | DatabasePreflight::Ready { .. } => None,
            DatabasePreflight::Quarantined { .. } => unreachable!("handled above"),
        };
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let mut connection =
            Connection::open_with_flags(path, flags).map_err(|_| storage_error())?;
        configure(&connection).map_err(|_| corrupt_storage_error())?;
        if migrations::migrations::runner()
            .run(&mut connection)
            .is_err()
        {
            drop(connection);
            if let Some(source_schema) = recovery_source_schema {
                MigrationRecovery::restore(path, source_schema, CURRENT_SCHEMA_VERSION)?;
            }
            return Err(corrupt_storage_error());
        }
        let schema_version = connection
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM refinery_schema_history",
                [],
                |row| row.get::<_, u32>(0),
            )
            .map_err(|_| corrupt_storage_error())?;
        if schema_version != CURRENT_SCHEMA_VERSION {
            return Err(corrupt_storage_error());
        }
        restrict_file_permissions(path)?;
        restrict_sqlite_sidecars(path)?;
        if let Some(recovery) = recovery {
            recovery.finish(schema_version)?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
            path: Some(path.to_path_buf()),
            _ownership: Some(ownership),
        })
    }

    /// Opens and migrates an isolated in-memory `SQLite` database.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when configuration or migration fails.
    pub fn in_memory() -> Result<Self, SafeError> {
        let mut connection = Connection::open_in_memory().map_err(|_| storage_error())?;
        configure(&connection)?;
        migrations::migrations::runner()
            .run(&mut connection)
            .map_err(|_| corrupt_storage_error())?;
        Ok(Self {
            connection: Mutex::new(connection),
            path: None,
            _ownership: None,
        })
    }

    /// Inspects schema and persisted identities without mutating the database.
    ///
    /// # Errors
    ///
    /// Returns a safe corrupt or unsupported-schema error when the database
    /// cannot be classified.
    pub fn preflight(path: &Path) -> Result<DatabasePreflight, SafeError> {
        preflight(path)
    }

    /// Verifies the authenticated, immutable backup retained for a migration.
    ///
    /// # Errors
    ///
    /// Returns a safe backup error when any manifest, digest, authentication
    /// tag, schema identity, or SQLite integrity check fails.
    pub fn verify_migration_backup(path: &Path, source_schema: u32) -> Result<(), SafeError> {
        MigrationRecovery::verify_evidence(path, source_schema, CURRENT_SCHEMA_VERSION)
    }

    /// Restores an authenticated pre-migration backup while retaining the
    /// displaced database as recovery evidence.
    ///
    /// # Errors
    ///
    /// Returns a safe storage or backup error without replacing the database
    /// when authentication or the atomic replacement fails.
    pub fn restore_migration_backup(path: &Path, source_schema: u32) -> Result<(), SafeError> {
        let _ownership = WritableOwnership::acquire(path)?;
        MigrationRecovery::restore(path, source_schema, CURRENT_SCHEMA_VERSION)
    }

    /// Exports a quarantined database without mutating it and authenticates
    /// the resulting SQLite artifact with a caller-owned repair capability.
    ///
    /// # Errors
    ///
    /// Returns a safe state, authorization, or storage error.
    pub fn export_quarantined(
        path: &Path,
        destination: &Path,
        authorization: &RepairAuthorization,
    ) -> Result<QuarantineExportReceipt, SafeError> {
        export_quarantined(path, destination, authorization)
    }

    /// Validates and authenticates a canonical repaired database candidate.
    ///
    /// # Errors
    ///
    /// Returns a safe compatibility or storage error for an invalid candidate.
    pub fn authenticate_repair_candidate(
        path: &Path,
        authorization: &RepairAuthorization,
    ) -> Result<RepairCandidate, SafeError> {
        authenticate_candidate(path, authorization)
    }

    /// Atomically installs an authenticated candidate over a quarantined
    /// database while retaining the original as immutable evidence.
    ///
    /// # Errors
    ///
    /// Returns a safe authorization, ownership, or storage error without
    /// replacing the target when any gate fails.
    pub fn install_repair_candidate(
        path: &Path,
        candidate: &RepairCandidate,
        authorization: &RepairAuthorization,
    ) -> Result<(), SafeError> {
        let _ownership = WritableOwnership::acquire(path)?;
        install_candidate(path, candidate, authorization)
    }

    /// Returns the highest successfully applied migration version.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when migration history cannot be read.
    pub fn schema_version(&self) -> Result<u32, SafeError> {
        self.connection()
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM refinery_schema_history",
                [],
                |row| row.get(0),
            )
            .map_err(|_| corrupt_storage_error())
    }

    pub(crate) fn connection(&self) -> DatabaseConnection<'_> {
        DatabaseConnection {
            connection: self
                .connection
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            path: self.path.as_deref(),
        }
    }
}

impl Deref for DatabaseConnection<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl DerefMut for DatabaseConnection<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}

impl Drop for DatabaseConnection<'_> {
    fn drop(&mut self) {
        if let Some(path) = self.path {
            let _ = restrict_sqlite_sidecars(path);
        }
    }
}

impl WritableOwnership {
    fn acquire(database_path: &Path) -> Result<Self, SafeError> {
        let lock_path = database_path.with_extension("sqlite3.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC).bits() as i32,
            );
        }
        let file = options.open(&lock_path).map_err(|_| storage_error())?;
        restrict_file_permissions(&lock_path)?;
        file.try_lock_exclusive().map_err(|_| ownership_error())?;
        Ok(Self { _file: file })
    }
}

fn create_secure_directory(path: &Path) -> Result<(), SafeError> {
    let mut existing = path;
    loop {
        match fs::symlink_metadata(existing) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(storage_error());
                }
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                existing = existing.parent().ok_or_else(storage_error)?;
            }
            Err(_) => return Err(storage_error()),
        }
    }
    fs::create_dir_all(path).map_err(|_| storage_error())?;
    let metadata = fs::symlink_metadata(path).map_err(|_| storage_error())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(storage_error());
    }
    restrict_directory_permissions(path)
}

fn configure(connection: &Connection) -> Result<(), SafeError> {
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .and_then(|()| connection.pragma_update(None, "trusted_schema", "OFF"))
        .and_then(|()| connection.pragma_update(None, "journal_mode", "WAL"))
        .and_then(|()| connection.pragma_update(None, "synchronous", "FULL"))
        .and_then(|()| connection.pragma_update(None, "secure_delete", "ON"))
        .and_then(|()| connection.pragma_update(None, "wal_autocheckpoint", 1_000))
        .and_then(|()| connection.busy_timeout(Duration::from_secs(5)))
        .map_err(|_| storage_error())
}

fn restrict_sqlite_sidecars(path: &Path) -> Result<(), SafeError> {
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        match fs::symlink_metadata(&sidecar) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(storage_error());
            }
            Ok(_) => restrict_file_permissions(&sidecar)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(storage_error()),
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn restrict_file_permissions(path: &Path) -> Result<(), SafeError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| storage_error())
}

#[cfg(unix)]
pub(crate) fn restrict_directory_permissions(path: &Path) -> Result<(), SafeError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| storage_error())
}

#[cfg(not(unix))]
pub(crate) fn restrict_file_permissions(_path: &Path) -> Result<(), SafeError> {
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn restrict_directory_permissions(_path: &Path) -> Result<(), SafeError> {
    Ok(())
}

const fn storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The application database is unavailable."),
    )
}

const fn corrupt_storage_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageCorrupt,
        SafeMessage::new("The application database could not be read."),
    )
}

const fn ownership_error() -> SafeError {
    SafeError::new(
        SafeErrorCode::StorageUnavailable,
        SafeMessage::new("The application database is already in use."),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use std::process::Command;

    use tempfile::tempdir;

    use radroots_studio_application::{AccountRepository, AppStateRepository};
    use radroots_studio_domain::{PublicKey, SafeErrorCode};
    use refinery::Target;
    use rusqlite::Connection;

    use super::{
        CURRENT_SCHEMA_VERSION, Database, configure, create_secure_directory, migrations,
        restrict_sqlite_sidecars,
    };
    use crate::{DatabasePreflight, PersistedIdentityIssueKind, RepairAuthorization};

    #[test]
    fn migration_opens_fresh_memory_database_once() {
        let database = Database::in_memory().expect("open memory database");

        assert_eq!(
            database.schema_version().expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
        assert_eq!(
            database.schema_version().expect("repeat schema version"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn database_path_guards_reject_files_as_directories_and_sidecars() {
        let directory = tempdir().expect("temporary directory");
        let regular = directory.path().join("regular");
        fs::write(&regular, b"file").expect("write regular file");
        assert!(create_secure_directory(&regular).is_err());

        let database = directory.path().join("studio.sqlite3");
        fs::write(&database, b"database").expect("write database file");
        fs::create_dir(directory.path().join("studio.sqlite3-wal"))
            .expect("create invalid WAL sidecar");
        assert!(restrict_sqlite_sidecars(&database).is_err());
    }

    #[test]
    fn sqlite_connection_enforces_trust_durability_and_busy_policy() {
        let database = Database::in_memory().expect("open memory database");
        let connection = database.connection();

        assert_eq!(
            connection
                .pragma_query_value(None, "foreign_keys", |row| row.get::<_, u8>(0))
                .expect("foreign keys"),
            1
        );
        assert_eq!(
            connection
                .pragma_query_value(None, "trusted_schema", |row| row.get::<_, u8>(0))
                .expect("trusted schema"),
            0
        );
        assert_eq!(
            connection
                .pragma_query_value(None, "synchronous", |row| row.get::<_, u8>(0))
                .expect("synchronous"),
            2
        );
        assert_eq!(
            connection
                .pragma_query_value(None, "busy_timeout", |row| row.get::<_, i64>(0))
                .expect("busy timeout"),
            5_000
        );
    }

    #[test]
    fn normalized_schema_is_strict_and_enforces_same_account_bindings() {
        let database = Database::in_memory().expect("open memory database");
        let connection = database.connection();
        let strict_tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_list WHERE name IN ('account_identities', 'local_signer_bindings', 'runtime_state', 'profile_cache_v6', 'durable_operations') AND strict = 1",
                [],
                |row| row.get(0),
            )
            .expect("strict table inventory");
        assert_eq!(strict_tables, 5);

        connection
            .execute(
                "INSERT INTO account_identities (public_key, npub, created_at) VALUES (?1, ?2, 1)",
                [
                    "07".repeat(32),
                    "npub1qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qursnvjvl7".to_owned(),
                ],
            )
            .expect("identity");
        assert!(
            connection
                .execute(
                    "INSERT INTO local_signer_bindings (account_public_key, binding_public_key, binding_kind, availability) VALUES (?1, ?2, 'local_secret', 'available')",
                    ["07".repeat(32), "08".repeat(32)],
                )
                .is_err()
        );
    }

    #[test]
    fn v5_data_migrates_append_only_with_identity_profile_and_selection() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        let public_key = "07".repeat(32);
        {
            let mut connection = Connection::open(&path).expect("legacy database");
            configure(&connection).expect("configuration");
            migrations::migrations::runner()
                .set_target(Target::Version(5))
                .run(&mut connection)
                .expect("V5 schema");
            connection
                .execute(
                    "INSERT INTO accounts (pubkey, npub, signer_kind, key_availability, created_at) VALUES (?1, ?2, 'local_secret', 'available', 10)",
                    [&public_key, "npub1qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qursnvjvl7"],
                )
                .expect("legacy account");
            connection
                .execute(
                    "UPDATE app_state SET selected_pubkey = ?1 WHERE singleton = 1",
                    [&public_key],
                )
                .expect("legacy selection");
            connection
                .execute(
                    "INSERT INTO profile_cache (subject_pubkey, event_id, event_created_at, name, refreshed_at, refresh_status) VALUES (?1, ?2, 11, 'Farm', 12, 'success')",
                    [&public_key, &"01".repeat(32)],
                )
                .expect("legacy profile");
        }

        let database = Database::open(&path).expect("migrated database");
        assert_eq!(database.schema_version().expect("version"), 10);
        assert_eq!(database.list_accounts().expect("accounts").len(), 1);
        assert_eq!(
            database.load_selected_account().expect("selection"),
            Some(PublicKey::from_bytes([7; 32]).expect("valid public key"))
        );
        let connection = database.connection();
        let migrated: (i64, i64, i64) = connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM account_identities), (SELECT COUNT(*) FROM local_signer_bindings), (SELECT COUNT(*) FROM profile_cache_v6)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("migrated inventory");
        assert_eq!(migrated, (1, 1, 1));
        drop(connection);
        drop(database);

        Database::verify_migration_backup(&path, 5).expect("authenticated backup");
        Database::restore_migration_backup(&path, 5).expect("authenticated restore");
        assert_eq!(
            Database::preflight(&path).expect("restored preflight"),
            DatabasePreflight::Ready { schema_version: 5 }
        );
        let retried = Database::open(&path).expect("idempotent migration retry");
        assert_eq!(retried.schema_version().expect("retried version"), 10);
        drop(retried);

        let backup = directory
            .path()
            .join("studio.sqlite3.recovery/migration-v5-to-v10.sqlite3");
        fs::OpenOptions::new()
            .append(true)
            .open(backup)
            .expect("open backup")
            .write_all(b"tamper")
            .expect("tamper backup");
        let error =
            Database::verify_migration_backup(&path, 5).expect_err("tampered backup must fail");
        assert_eq!(error.code(), SafeErrorCode::StorageBackupInvalid);
    }

    #[test]
    fn corrupt_v5_identity_fails_before_migration_without_recreation() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        {
            let mut connection = Connection::open(&path).expect("legacy database");
            configure(&connection).expect("configuration");
            migrations::migrations::runner()
                .set_target(Target::Version(5))
                .run(&mut connection)
                .expect("V5 schema");
            connection
                .execute(
                    "INSERT INTO accounts (pubkey, npub, signer_kind, key_availability, created_at) VALUES (?1, ?2, 'local_secret', 'available', 10)",
                    ["07".repeat(32), "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg".to_owned()],
                )
                .expect("mismatched legacy account");
        }

        assert!(Database::open(&path).is_err());
        let connection = Connection::open(&path).expect("inspect legacy database");
        let version: u32 = connection
            .query_row(
                "SELECT MAX(version) FROM refinery_schema_history",
                [],
                |row| row.get(0),
            )
            .expect("legacy version");
        let accounts: i64 = connection
            .query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))
            .expect("legacy accounts");
        assert_eq!((version, accounts), (5, 1));
    }

    #[test]
    fn invalid_curve_identity_is_quarantined_without_mutation() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        {
            let mut connection = Connection::open(&path).expect("legacy database");
            configure(&connection).expect("configuration");
            migrations::migrations::runner()
                .set_target(Target::Version(5))
                .run(&mut connection)
                .expect("V5 schema");
            connection
                .execute(
                    "INSERT INTO accounts (pubkey, npub, signer_kind, key_availability, created_at) VALUES (?1, ?2, 'local_secret', 'available', 10)",
                    ["00".repeat(32), "npub1qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qursnvjvl7".to_owned()],
                )
                .expect("invalid-curve fixture");
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                .expect("checkpoint");
        }
        let before = fs::read(&path).expect("before bytes");

        let DatabasePreflight::Quarantined {
            schema_version,
            issues,
        } = Database::preflight(&path).expect("classified preflight")
        else {
            panic!("invalid identity was not quarantined");
        };
        assert_eq!(schema_version, 5);
        assert!(issues.iter().any(|issue| {
            issue.table() == "accounts"
                && issue.column() == "pubkey"
                && issue.kind() == PersistedIdentityIssueKind::InvalidCurvePoint
        }));
        let error = Database::open(&path)
            .err()
            .expect("quarantined open must fail");
        assert_eq!(error.code(), SafeErrorCode::StorageQuarantined);
        assert_eq!(fs::read(&path).expect("after bytes"), before);
        assert!(!path.with_extension("sqlite3.lock").exists());

        let authorization = RepairAuthorization::from_bytes(vec![0x41; 32])
            .unwrap_or_else(|_| panic!("repair authorization"));
        let export_path = directory.path().join("quarantine-export.sqlite3");
        let export = Database::export_quarantined(&path, &export_path, &authorization)
            .expect("authenticated quarantine export");
        assert_eq!(export.path(), export_path);
        assert_eq!(export.sha256().len(), 64);
        assert_eq!(export.authentication_tag().len(), 64);
        assert_eq!(fs::read(&path).expect("post-export bytes"), before);

        let candidate_path = directory.path().join("repaired.sqlite3");
        drop(Database::open(&candidate_path).expect("canonical repair candidate"));
        let candidate = Database::authenticate_repair_candidate(&candidate_path, &authorization)
            .expect("authenticate candidate");
        let wrong_authorization = RepairAuthorization::from_bytes(vec![0x42; 32])
            .unwrap_or_else(|_| panic!("wrong authorization shape"));
        let error = Database::install_repair_candidate(&path, &candidate, &wrong_authorization)
            .expect_err("wrong repair authorization");
        assert_eq!(error.code(), SafeErrorCode::RepairUnauthorized);
        assert_eq!(fs::read(&path).expect("unauthorized bytes"), before);

        Database::install_repair_candidate(&path, &candidate, &authorization)
            .expect("authenticated repair install");
        assert!(matches!(
            Database::preflight(&path).expect("repaired preflight"),
            DatabasePreflight::Ready {
                schema_version: CURRENT_SCHEMA_VERSION
            }
        ));
        assert!(
            directory
                .path()
                .join("studio.sqlite3.quarantined-evidence")
                .is_file()
        );
    }

    #[test]
    fn newer_and_mixed_schema_inventory_fail_before_mutation() {
        let directory = tempdir().expect("temporary directory");
        let newer_path = directory.path().join("newer.sqlite3");
        {
            let database = Database::open(&newer_path).expect("current database");
            database
                .connection()
                .execute(
                    "UPDATE refinery_schema_history SET version = ?1 WHERE version = ?2",
                    [CURRENT_SCHEMA_VERSION + 1, CURRENT_SCHEMA_VERSION],
                )
                .expect("future schema row");
        }
        let newer_before = fs::read(&newer_path).expect("newer bytes");
        let error = Database::preflight(&newer_path).expect_err("newer schema");
        assert_eq!(error.code(), SafeErrorCode::UnsupportedSchemaVersion);
        assert_eq!(fs::read(&newer_path).expect("newer after"), newer_before);

        let mixed_path = directory.path().join("mixed.sqlite3");
        {
            let mut connection = Connection::open(&mixed_path).expect("legacy database");
            configure(&connection).expect("configuration");
            migrations::migrations::runner()
                .set_target(Target::Version(5))
                .run(&mut connection)
                .expect("V5 schema");
            connection
                .execute("CREATE TABLE installation_identity (singleton INTEGER)", [])
                .expect("mixed table");
        }
        let mixed_before = fs::read(&mixed_path).expect("mixed bytes");
        let error = Database::preflight(&mixed_path).expect_err("mixed schema");
        assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
        assert_eq!(fs::read(&mixed_path).expect("mixed after"), mixed_before);
    }

    #[test]
    fn failed_v5_copy_rolls_back_the_active_migration() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        let public_key = "07".repeat(32);
        {
            let mut connection = Connection::open(&path).expect("legacy database");
            configure(&connection).expect("configuration");
            migrations::migrations::runner()
                .set_target(Target::Version(5))
                .run(&mut connection)
                .expect("V5 schema");
            connection
                .execute(
                    "INSERT INTO accounts (pubkey, npub, signer_kind, key_availability, created_at) VALUES (?1, ?2, 'local_secret', 'available', 10)",
                    [&public_key, "npub1qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qurswpc8qursnvjvl7"],
                )
                .expect("legacy account");
            connection
                .execute(
                    "INSERT INTO profile_cache (subject_pubkey, event_id, event_created_at, refreshed_at, refresh_status) VALUES (?1, 'invalid', 11, 12, 'success')",
                    [&public_key],
                )
                .expect("legacy corrupt profile");
        }

        assert!(Database::open(&path).is_err());
        let connection = Connection::open(&path).expect("inspect interrupted migration");
        let version: u32 = connection
            .query_row(
                "SELECT MAX(version) FROM refinery_schema_history",
                [],
                |row| row.get(0),
            )
            .expect("migration version");
        assert_eq!(version, 5);
        assert!(!connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'account_identities')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .expect("normalized table inventory"));
    }

    #[test]
    fn foreign_keys_reject_orphan_normalized_records() {
        let database = Database::in_memory().expect("database");
        let connection = database.connection();
        assert!(
            connection
                .execute(
                    "INSERT INTO local_signer_bindings (account_public_key, binding_public_key, binding_kind, availability) VALUES (?1, ?1, 'local_secret', 'available')",
                    ["09".repeat(32)],
                )
                .is_err()
        );
    }

    #[test]
    fn second_process_cannot_acquire_writable_ownership() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        let _owner = Database::open(&path).expect("parent owner");
        let status = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("db::tests::writable_ownership_child_probe")
            .arg("--nocapture")
            .env("RADROOTS_STUDIO_LOCK_PROBE_PATH", &path)
            .status()
            .expect("child process");
        assert!(status.success());
    }

    #[test]
    fn writable_ownership_child_probe() {
        let Ok(path) = std::env::var("RADROOTS_STUDIO_LOCK_PROBE_PATH") else {
            return;
        };
        assert!(Database::open(Path::new(&path)).is_err());
    }

    #[test]
    fn migration_persists_schema_version_across_file_reopen() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");

        {
            let database = Database::open(&path).expect("open file database");
            assert_eq!(
                database.schema_version().expect("schema version"),
                CURRENT_SCHEMA_VERSION
            );
        }
        let reopened = Database::open(&path).expect("reopen file database");
        assert_eq!(
            reopened.schema_version().expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
        assert!(fs::metadata(path).expect("database metadata").len() > 0);
    }

    #[test]
    fn writable_ownership_rejects_a_second_runtime_and_releases_on_drop() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        let first = Database::open(&path).expect("first owner");
        let Err(error) = Database::open(&path) else {
            panic!("second owner must fail");
        };
        assert_eq!(
            error.message().as_str(),
            "The application database is already in use."
        );
        drop(first);
        Database::open(&path).expect("ownership released");
    }

    #[cfg(unix)]
    #[test]
    fn migration_attempts_owner_only_database_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        let database = Database::open(&path).expect("open file database");
        let mode = fs::metadata(&path)
            .expect("database metadata")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(mode, 0o600);
        let directory_mode = fs::metadata(directory.path())
            .expect("directory metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);

        let connection = database.connection();
        connection
            .execute_batch("CREATE TABLE sidecar_probe (value INTEGER) STRICT; INSERT INTO sidecar_probe VALUES (1);")
            .expect("write through WAL");
        drop(connection);
        for suffix in ["-wal", "-shm"] {
            let sidecar = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
            let sidecar_mode = fs::metadata(sidecar)
                .expect("sidecar metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(sidecar_mode, 0o600);
        }
    }

    #[cfg(unix)]
    #[test]
    fn database_lock_sidecar_and_recovery_symlinks_fail_closed() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("temporary directory");
        let victim = directory.path().join("victim");
        fs::write(&victim, b"unchanged").expect("victim");

        let database_link = directory.path().join("database-link.sqlite3");
        symlink(&victim, &database_link).expect("database symlink");
        assert!(Database::open(&database_link).is_err());
        assert_eq!(fs::read(&victim).expect("victim bytes"), b"unchanged");

        let lock_path = directory.path().join("locked.sqlite3");
        symlink(&victim, lock_path.with_extension("sqlite3.lock")).expect("lock symlink");
        assert!(Database::open(&lock_path).is_err());
        assert_eq!(fs::read(&victim).expect("victim bytes"), b"unchanged");

        let sidecar_path = directory.path().join("sidecar.sqlite3");
        let wal = std::path::PathBuf::from(format!("{}-wal", sidecar_path.display()));
        symlink(&victim, wal).expect("WAL symlink");
        assert!(Database::open(&sidecar_path).is_err());
        assert_eq!(fs::read(&victim).expect("victim bytes"), b"unchanged");

        let legacy_path = directory.path().join("legacy.sqlite3");
        {
            let mut connection = Connection::open(&legacy_path).expect("legacy database");
            configure(&connection).expect("configuration");
            migrations::migrations::runner()
                .set_target(Target::Version(5))
                .run(&mut connection)
                .expect("V5 schema");
        }
        symlink(
            directory.path().join("not-present"),
            directory.path().join("legacy.sqlite3.recovery"),
        )
        .expect("recovery symlink");
        assert!(Database::open(&legacy_path).is_err());
    }
}
