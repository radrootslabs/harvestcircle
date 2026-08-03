use std::path::Path;

use radroots_studio_application::{
    AppCore, AppSnapshot, Clock, DurableRequestId, GenerateAccountReceipt, ImportAccountReceipt,
    RelayConfiguration, RemovalConfirmationToken, SecretStore,
};
use radroots_studio_domain::{PublicKey, SafeError, SecretKeyInput};

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
    pub fn bootstrap(
        &self,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.core.recover_pending_operations(
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )?;
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

    /// Generates an account through the durable request coordinator.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict, credential, storage, or application-state error.
    pub fn generate_account_durable(
        &self,
        request_id: &DurableRequestId,
        expected_revision: u64,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<GenerateAccountReceipt, SafeError> {
        self.core.generate_account_durable(
            request_id,
            expected_revision,
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )
    }

    /// Imports or repairs an account through the durable request coordinator.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict, validation, credential, storage, or state error.
    pub fn import_secret_key_durable(
        &self,
        request_id: &DurableRequestId,
        expected_revision: u64,
        input: SecretKeyInput,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportAccountReceipt, SafeError> {
        self.core.import_secret_key_durable(
            request_id,
            expected_revision,
            input,
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )
    }

    /// Persists and publishes one saved-account selection without activation.
    ///
    /// # Errors
    ///
    /// Returns a safe account, storage, or application-state error.
    pub fn select_account(&self, public_key: PublicKey) -> Result<AppSnapshot, SafeError> {
        self.core
            .select_account(public_key, &self.database, &self.database)
    }

    /// Activates a saved account after validating its credential and cached profile.
    ///
    /// # Errors
    ///
    /// Returns a safe account, credential, storage, or application-state error.
    pub fn activate_account(
        &self,
        public_key: PublicKey,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.core.activate_account(
            public_key,
            &self.database,
            &self.database,
            &self.database,
            secrets,
            clock,
        )
    }

    /// Signs out while retaining durable account data and credentials.
    ///
    /// # Errors
    ///
    /// Returns a safe application-state error if sign out cannot complete.
    pub fn sign_out(&self) -> Result<AppSnapshot, SafeError> {
        self.core.sign_out()
    }

    /// Issues a revision-bound, single-use account-removal confirmation.
    ///
    /// # Errors
    ///
    /// Returns a safe error when the target account is not saved.
    pub fn request_account_removal(
        &self,
        public_key: PublicKey,
    ) -> Result<RemovalConfirmationToken, SafeError> {
        self.core.request_account_removal(public_key)
    }

    /// Permanently removes one confirmed account and its credential.
    ///
    /// # Errors
    ///
    /// Returns a safe confirmation, credential, storage, recovery, or state error.
    pub fn confirm_account_removal(
        &self,
        token: RemovalConfirmationToken,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.core.confirm_account_removal(
            token,
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
        AccountOperationKind, AccountOperationPhase, AccountRepository, AppLifecycle,
        AppStateRepository, Clock, DurableOperationRepository, DurableRequestId,
        FailureSecretStore, InMemorySecretStore, OperationJournal, RelayConfiguration, SecretStore,
        SecretStoreOperation, SessionState,
    };
    use radroots_studio_domain::{
        AccountCreatedAt, AccountIdentity, AccountSummary, BindingAvailability, LocalSignerBinding,
        PublicKey, SafeErrorCode, SecretKeyInput, UnixTimestamp,
    };
    use tempfile::tempdir;

    use super::PersistentAppCore;

    fn account() -> AccountSummary {
        let public_key = PublicKey::from_bytes([4; 32]);
        AccountSummary::new(
            AccountIdentity::derive(public_key).expect("identity"),
            LocalSignerBinding::new(public_key, BindingAvailability::Available),
            None,
            AccountCreatedAt::new(UnixTimestamp::from_seconds(1).expect("time")),
            None,
        )
        .expect("account")
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
        let secrets = InMemorySecretStore::default();
        {
            let adapter = PersistentAppCore::open(&path, RelayConfiguration::default())
                .expect("open adapter");
            let fresh = adapter
                .bootstrap(&secrets, &FixedClock)
                .expect("fresh bootstrap");
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
        let restored = adapter.bootstrap(&secrets, &FixedClock).expect("restore");
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
            adapter.bootstrap(&secrets, &FixedClock).expect("bootstrap");
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
        let restored = reopened.bootstrap(&secrets, &FixedClock).expect("restore");
        assert_eq!(restored.accounts().len(), 2);
        assert_eq!(restored.selected_account(), Some(selected));
        assert_eq!(restored.session(), SessionState::SignedOut);
    }

    #[test]
    fn durable_import_commits_each_phase_and_recovers_the_terminal_receipt() {
        let adapter = PersistentAppCore::in_memory(RelayConfiguration::default()).expect("adapter");
        let secrets = InMemorySecretStore::default();
        let snapshot = adapter.bootstrap(&secrets, &FixedClock).expect("bootstrap");
        let request = DurableRequestId::parse("import:adapter:1").expect("request");
        let imported = adapter
            .import_secret_key_durable(
                &request,
                snapshot.revision().value(),
                SecretKeyInput::parse(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_owned(),
                )
                .expect("secret"),
                &secrets,
                &FixedClock,
            )
            .expect("durable import");
        let operation = adapter
            .database()
            .load_durable_operation(&request)
            .expect("operation")
            .expect("durable record");
        let receipt = operation.terminal().expect("terminal receipt");
        assert_eq!(receipt.account(), imported.account().public_key());
        assert_eq!(
            receipt.resulting_revision(),
            Some(adapter.core().snapshot().revision().value())
        );
    }

    #[test]
    fn bootstrap_recovery_completes_credential_deleted_removal_and_fallback() {
        let directory = tempdir().expect("directory");
        let path = directory.path().join("studio.sqlite3");
        let secrets = InMemorySecretStore::default();
        let first;
        let removed;
        {
            let adapter =
                PersistentAppCore::open(&path, RelayConfiguration::default()).expect("adapter");
            adapter.bootstrap(&secrets, &FixedClock).expect("bootstrap");
            first = adapter
                .generate_account(&secrets, &FixedClock)
                .expect("first")
                .account()
                .public_key();
            removed = adapter
                .generate_account(&secrets, &FixedClock)
                .expect("removed")
                .account()
                .public_key();
            let operation = adapter
                .database()
                .begin_operation(AccountOperationKind::Remove, removed, FixedClock.now())
                .expect("intent");
            secrets.delete(removed).expect("credential deletion");
            adapter
                .database()
                .update_operation(
                    operation,
                    AccountOperationPhase::CredentialDeleted,
                    FixedClock.now(),
                    None,
                )
                .expect("phase");
        }

        let reopened =
            PersistentAppCore::open(&path, RelayConfiguration::default()).expect("reopen");
        let restored = reopened
            .bootstrap(&secrets, &FixedClock)
            .expect("recover and bootstrap");
        assert_eq!(restored.accounts().len(), 1);
        assert_eq!(restored.selected_account(), Some(first));
        assert_eq!(restored.session(), SessionState::SignedOut);
        assert!(
            reopened
                .database()
                .list_pending_operations()
                .expect("journal")
                .is_empty()
        );
        assert!(
            reopened
                .database()
                .find_account(removed)
                .expect("removed")
                .is_none()
        );
    }

    #[test]
    fn bootstrap_skips_keyring_when_journal_empty_and_retains_failed_intent() {
        let empty = PersistentAppCore::in_memory(RelayConfiguration::default()).expect("empty");
        let unavailable = FailureSecretStore::default();
        unavailable.fail_next(SecretStoreOperation::Delete);
        empty
            .bootstrap(&unavailable, &FixedClock)
            .expect("empty journal does not access keyring");

        let adapter = PersistentAppCore::in_memory(RelayConfiguration::default()).expect("adapter");
        adapter
            .database()
            .insert_account(&account())
            .expect("account");
        adapter
            .database()
            .save_selected_account(Some(account().public_key()))
            .expect("selection");
        adapter
            .database()
            .begin_operation(
                AccountOperationKind::Remove,
                account().public_key(),
                FixedClock.now(),
            )
            .expect("intent");
        let failing = FailureSecretStore::default();
        failing.fail_next(SecretStoreOperation::Delete);
        let error = adapter
            .bootstrap(&failing, &FixedClock)
            .expect_err("keyring unavailable");
        assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
        let pending = adapter
            .database()
            .list_pending_operations()
            .expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].phase(), AccountOperationPhase::IntentRecorded);
    }
}
