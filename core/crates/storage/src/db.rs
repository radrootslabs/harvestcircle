use std::fs::{self, File, OpenOptions};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use fs2::FileExt;
use radroots_studio_domain::{AccountIdentity, PublicKey, SafeError, SafeErrorCode, SafeMessage};
use refinery::embed_migrations;
use rusqlite::{Connection, OpenFlags};

pub const CURRENT_SCHEMA_VERSION: u32 = 9;

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
        let parent = path.parent().ok_or_else(storage_error)?;
        create_secure_directory(parent)?;
        let ownership = WritableOwnership::acquire(path)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut connection =
            Connection::open_with_flags(path, flags).map_err(|_| storage_error())?;
        configure(&connection).map_err(|_| corrupt_storage_error())?;
        validate_legacy_account_identities(&connection)?;
        migrations::migrations::runner()
            .run(&mut connection)
            .map_err(|_| corrupt_storage_error())?;
        restrict_file_permissions(path)?;
        restrict_sqlite_sidecars(path)?;
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
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| storage_error())?;
        restrict_file_permissions(&lock_path)?;
        file.try_lock_exclusive().map_err(|_| ownership_error())?;
        Ok(Self { _file: file })
    }
}

fn create_secure_directory(path: &Path) -> Result<(), SafeError> {
    fs::create_dir_all(path).map_err(|_| storage_error())?;
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

fn validate_legacy_account_identities(connection: &Connection) -> Result<(), SafeError> {
    let has_accounts = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'accounts')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| corrupt_storage_error())?;
    if !has_accounts {
        return Ok(());
    }

    let mut statement = connection
        .prepare("SELECT pubkey, npub, signer_kind, key_availability FROM accounts")
        .map_err(|_| corrupt_storage_error())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| corrupt_storage_error())?;
    for row in rows {
        let (public_key, npub, signer_kind, availability) =
            row.map_err(|_| corrupt_storage_error())?;
        let public_key = PublicKey::from_hex(&public_key).map_err(|_| corrupt_storage_error())?;
        AccountIdentity::verify(public_key, npub).map_err(|_| corrupt_storage_error())?;
        if signer_kind != "local_secret"
            || !matches!(
                availability.as_str(),
                "available" | "credential_missing" | "store_unavailable"
            )
        {
            return Err(corrupt_storage_error());
        }
    }
    Ok(())
}

fn restrict_sqlite_sidecars(path: &Path) -> Result<(), SafeError> {
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if sidecar.exists() {
            restrict_file_permissions(&sidecar)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> Result<(), SafeError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| storage_error())
}

#[cfg(unix)]
fn restrict_directory_permissions(path: &Path) -> Result<(), SafeError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| storage_error())
}

#[cfg(not(unix))]
fn restrict_file_permissions(_path: &Path) -> Result<(), SafeError> {
    Ok(())
}

#[cfg(not(unix))]
fn restrict_directory_permissions(_path: &Path) -> Result<(), SafeError> {
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
    use std::path::Path;
    use std::process::Command;

    use tempfile::tempdir;

    use radroots_studio_application::{AccountRepository, AppStateRepository};
    use radroots_studio_domain::PublicKey;
    use refinery::Target;
    use rusqlite::Connection;

    use super::{CURRENT_SCHEMA_VERSION, Database, configure, migrations};

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
        assert_eq!(database.schema_version().expect("version"), 9);
        assert_eq!(database.list_accounts().expect("accounts").len(), 1);
        assert_eq!(
            database.load_selected_account().expect("selection"),
            Some(PublicKey::from_bytes([7; 32]))
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
        let copied: i64 = connection
            .query_row("SELECT COUNT(*) FROM account_identities", [], |row| {
                row.get(0)
            })
            .expect("normalized accounts");
        assert_eq!((version, copied), (6, 0));
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
}
