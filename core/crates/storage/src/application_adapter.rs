use std::path::Path;

use radroots_studio_application::{AppCore, AppSnapshot, RelayConfiguration};
use radroots_studio_domain::SafeError;

use crate::Database;

pub struct PersistentAppCore {
    core: AppCore,
    database: Database,
}

impl PersistentAppCore {
    /// Opens the application database without accessing credentials or relays.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the database cannot be opened or migrated.
    pub fn open(path: &Path, relay_configuration: RelayConfiguration) -> Result<Self, SafeError> {
        Ok(Self {
            core: AppCore::in_memory(relay_configuration),
            database: Database::open(path)?,
        })
    }

    /// Creates an isolated persistent-core adapter for tests.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the database cannot be initialized.
    pub fn in_memory(relay_configuration: RelayConfiguration) -> Result<Self, SafeError> {
        Ok(Self {
            core: AppCore::in_memory(relay_configuration),
            database: Database::in_memory()?,
        })
    }

    /// Restores public accounts and selection while keeping the session signed out.
    ///
    /// # Errors
    ///
    /// Returns a safe storage or application-state error after publishing a fatal
    /// snapshot when durable state cannot be restored.
    pub fn bootstrap(&self) -> Result<AppSnapshot, SafeError> {
        self.core.bootstrap_from(&self.database, &self.database)
    }

    #[must_use]
    pub const fn core(&self) -> &AppCore {
        &self.core
    }

    #[must_use]
    pub const fn database(&self) -> &Database {
        &self.database
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use radroots_studio_application::{
        AccountRepository, AppLifecycle, AppStateRepository, RelayConfiguration, SessionState,
    };
    use radroots_studio_domain::{
        AccountCreatedAt, AccountSummary, KeyAvailability, Npub, PublicKey, SafeErrorCode,
        SignerKind, UnixTimestamp,
    };
    use tempfile::tempdir;

    use super::PersistentAppCore;

    const NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

    fn account() -> AccountSummary {
        AccountSummary::new(
            PublicKey::from_bytes([4; 32]),
            Npub::from_encoded(NPUB.to_owned()).expect("npub"),
            SignerKind::LocalSecret,
            KeyAvailability::Available,
            None,
            AccountCreatedAt::new(UnixTimestamp::from_seconds(1).expect("time")),
            None,
        )
    }

    #[test]
    fn persistent_bootstrap_handles_fresh_and_existing_signed_out_state() {
        let directory = tempdir().expect("directory");
        let path = directory.path().join("studio.sqlite3");
        let public_key = account().public_key();
        {
            let adapter = PersistentAppCore::open(&path, RelayConfiguration::default())
                .expect("open adapter");
            let fresh = adapter.bootstrap().expect("fresh bootstrap");
            assert!(fresh.accounts().is_empty());
            adapter
                .database()
                .insert_account(&account())
                .expect("account");
            adapter
                .database()
                .save_selected_account(Some(public_key))
                .expect("selection");
        }

        let adapter =
            PersistentAppCore::open(&path, RelayConfiguration::default()).expect("reopen adapter");
        let restored = adapter.bootstrap().expect("restore");
        assert_eq!(restored.lifecycle(), AppLifecycle::Ready);
        assert_eq!(restored.accounts().len(), 1);
        assert_eq!(restored.selected_account(), Some(public_key));
        assert_eq!(restored.session(), SessionState::SignedOut);
        assert!(restored.active_account().is_none());
    }

    #[test]
    fn corrupt_database_fails_safely_without_recreation() {
        let directory = tempdir().expect("directory");
        let path = directory.path().join("studio.sqlite3");
        fs::write(&path, b"not a sqlite database").expect("corrupt file");

        let error = PersistentAppCore::open(&path, RelayConfiguration::default())
            .err()
            .expect("safe failure");
        assert_eq!(error.code(), SafeErrorCode::StorageCorrupt);
        assert_eq!(
            fs::read(&path).expect("unchanged file"),
            b"not a sqlite database"
        );
    }
}
