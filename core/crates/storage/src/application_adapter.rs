use std::path::Path;

use radroots_studio_application::{
    AppCore, AppSnapshot, Clock, GenerateAccountReceipt, ImportAccountReceipt, RelayConfiguration,
    SecretStore,
};
use radroots_studio_domain::{SafeError, SecretKeyInput};

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

    /// Generates and durably persists one selected, signed-out local account.
    ///
    /// # Errors
    ///
    /// Returns a safe credential, storage, key, or application-state error.
    pub fn generate_account(
        &self,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<GenerateAccountReceipt, SafeError> {
        self.core.generate_account(
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )
    }

    /// Imports and durably persists one selected, signed-out local account.
    ///
    /// # Errors
    ///
    /// Returns a safe credential, storage, key, or application-state error.
    pub fn import_secret_key(
        &self,
        input: SecretKeyInput,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportAccountReceipt, SafeError> {
        self.core.import_secret_key(
            input,
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )
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
        AccountRepository, AppLifecycle, AppStateRepository, Clock, InMemorySecretStore,
        RelayConfiguration, SecretStore, SessionState,
    };
    use radroots_studio_domain::{
        AccountCreatedAt, AccountSummary, KeyAvailability, Npub, PublicKey, SafeErrorCode,
        SecretKeyInput, SignerKind, UnixTimestamp,
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

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(25).expect("time")
        }
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

    #[test]
    fn persisted_generate_and_import_survive_restart_without_secret_bytes() {
        let directory = tempdir().expect("directory");
        let path = directory.path().join("studio.sqlite3");
        let secrets = InMemorySecretStore::default();
        let selected;
        {
            let adapter =
                PersistentAppCore::open(&path, RelayConfiguration::default()).expect("adapter");
            adapter.bootstrap().expect("bootstrap");
            let generated = adapter
                .generate_account(&secrets, &FixedClock)
                .expect("generate");
            assert!(
                secrets
                    .contains(generated.account().public_key())
                    .expect("generated credential")
            );
            let imported = adapter
                .import_secret_key(
                    SecretKeyInput::parse(
                        "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"
                            .to_owned(),
                    )
                    .expect("secret"),
                    &secrets,
                    &FixedClock,
                )
                .expect("import");
            selected = imported.account().public_key();
            assert_eq!(adapter.core().snapshot().accounts().len(), 2);
        }

        let bytes = fs::read(&path).expect("database bytes");
        assert!(!bytes.windows(5).any(|value| value == b"nsec1"));
        assert!(!bytes.windows(64).any(|value| {
            value == b"7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"
        }));
        let reopened =
            PersistentAppCore::open(&path, RelayConfiguration::default()).expect("reopen");
        let restored = reopened.bootstrap().expect("restore");
        assert_eq!(restored.accounts().len(), 2);
        assert_eq!(restored.selected_account(), Some(selected));
        assert_eq!(restored.session(), SessionState::SignedOut);
    }
}
