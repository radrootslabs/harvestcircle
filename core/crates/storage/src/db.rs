use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use radroots_studio_domain::{SafeError, SafeErrorCode, SafeMessage};
use refinery::embed_migrations;
use rusqlite::{Connection, OpenFlags};

mod migrations {
    use super::embed_migrations;

    embed_migrations!("migrations");
}

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    /// Opens, configures, and migrates a file-backed `SQLite` database.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the file, connection configuration,
    /// permission update, or migration cannot complete.
    pub fn open(path: &Path) -> Result<Self, SafeError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut connection =
            Connection::open_with_flags(path, flags).map_err(|_| storage_error())?;
        configure(&connection)?;
        migrations::migrations::runner()
            .run(&mut connection)
            .map_err(|_| corrupt_storage_error())?;
        restrict_file_permissions(path)?;
        Ok(Self {
            connection: Mutex::new(connection),
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

    pub(crate) fn connection(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn configure(connection: &Connection) -> Result<(), SafeError> {
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .and_then(|()| connection.pragma_update(None, "journal_mode", "WAL"))
        .and_then(|()| connection.busy_timeout(Duration::from_secs(5)))
        .map_err(|_| storage_error())
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> Result<(), SafeError> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| storage_error())
}

#[cfg(not(unix))]
fn restrict_file_permissions(_path: &Path) -> Result<(), SafeError> {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::Database;

    #[test]
    fn migration_opens_fresh_memory_database_once() {
        let database = Database::in_memory().expect("open memory database");

        assert_eq!(database.schema_version().expect("schema version"), 1);
        assert_eq!(database.schema_version().expect("repeat schema version"), 1);
    }

    #[test]
    fn migration_persists_schema_version_across_file_reopen() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");

        {
            let database = Database::open(&path).expect("open file database");
            assert_eq!(database.schema_version().expect("schema version"), 1);
        }
        let reopened = Database::open(&path).expect("reopen file database");
        assert_eq!(reopened.schema_version().expect("schema version"), 1);
        assert!(fs::metadata(path).expect("database metadata").len() > 0);
    }

    #[cfg(unix)]
    #[test]
    fn migration_attempts_owner_only_database_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("studio.sqlite3");
        let _database = Database::open(&path).expect("open file database");
        let mode = fs::metadata(path)
            .expect("database metadata")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(mode, 0o600);
    }
}
