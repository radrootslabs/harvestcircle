use std::path::Path;
use std::sync::Arc;

use harvestcircle_application::{
    AppCore, AppSnapshot, Clock, DurableRequestId, GenerateIdentityReceipt, ImportIdentityReceipt,
    KeyMaterialProvider, RelayConfiguration, RemovalConfirmationToken, SecretStore,
    StagedGeneratedKey,
};
use harvestcircle_domain::{PublicKey, SafeError, SecretKeyInput};
use harvestcircle_nostr::NostrKeyMaterialProvider;

use harvestcircle_storage::Database;

use crate::{InstallationIdentity, InstallationIdentitySource};

pub struct PersistentAppCore {
    core: AppCore,
    database: Database,
    key_material: Arc<dyn KeyMaterialProvider>,
}

impl PersistentAppCore {
    pub(crate) fn initialize_installation_identity(
        &self,
        source: &dyn InstallationIdentitySource,
    ) -> Result<InstallationIdentity, SafeError> {
        if let Some(existing) = self.database.load_installation_id()? {
            return InstallationIdentity::parse(existing);
        }
        let candidate = source.generate()?;
        InstallationIdentity::parse(
            self.database
                .initialize_installation_id(candidate.as_str())?,
        )
    }

    /// Commits an acknowledged generated-key stage through the durable coordinator.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict, credential, storage, or recovery error.
    pub fn commit_staged_generated_key(
        &self,
        request_id: &DurableRequestId,
        staged: StagedGeneratedKey,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportIdentityReceipt, SafeError> {
        self.core.commit_staged_generated_key(
            request_id,
            staged,
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )
    }

    /// Opens the application database without accessing credentials or relays.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the database cannot be opened or migrated.
    pub fn open(path: &Path, relay_configuration: RelayConfiguration) -> Result<Self, SafeError> {
        let key_material: Arc<dyn KeyMaterialProvider> = Arc::new(NostrKeyMaterialProvider);
        Ok(Self {
            core: AppCore::new(relay_configuration, Arc::clone(&key_material)),
            database: Database::open(path)?,
            key_material,
        })
    }

    /// Creates an isolated persistent-core adapter for tests.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the database cannot be initialized.
    pub fn in_memory(relay_configuration: RelayConfiguration) -> Result<Self, SafeError> {
        let key_material: Arc<dyn KeyMaterialProvider> = Arc::new(NostrKeyMaterialProvider);
        Ok(Self {
            core: AppCore::new(relay_configuration, Arc::clone(&key_material)),
            database: Database::in_memory()?,
            key_material,
        })
    }

    pub(crate) fn key_material(&self) -> &dyn KeyMaterialProvider {
        self.key_material.as_ref()
    }

    /// Restores public identities and selection while keeping the session signed out.
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
        self.core.recover_durable_operations(
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )?;
        self.core.recover_pending_operations(
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )?;
        self.core.bootstrap_from(&self.database, &self.database)
    }

    /// Generates and durably persists one selected, signed-out local identity.
    ///
    /// # Errors
    ///
    /// Returns a safe credential, storage, key, or application-state error.
    pub fn generate_identity(
        &self,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<GenerateIdentityReceipt, SafeError> {
        self.core.generate_identity(
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )
    }

    /// Imports and durably persists one selected, signed-out local identity.
    ///
    /// # Errors
    ///
    /// Returns a safe credential, storage, key, or application-state error.
    pub fn import_secret_key(
        &self,
        input: SecretKeyInput,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportIdentityReceipt, SafeError> {
        self.core.import_secret_key(
            input,
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )
    }

    /// Generates an identity through the durable request coordinator.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict, credential, storage, or application-state error.
    pub fn generate_identity_durable(
        &self,
        request_id: &DurableRequestId,
        expected_revision: u64,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<GenerateIdentityReceipt, SafeError> {
        self.core.generate_identity_durable(
            request_id,
            expected_revision,
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )
    }

    /// Imports or repairs an identity through the durable request coordinator.
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
    ) -> Result<ImportIdentityReceipt, SafeError> {
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

    /// Persists and publishes one saved-identity selection without activation.
    ///
    /// # Errors
    ///
    /// Returns a safe identity, storage, or application-state error.
    pub fn select_identity(&self, public_key: PublicKey) -> Result<AppSnapshot, SafeError> {
        self.core
            .select_identity(public_key, &self.database, &self.database)
    }

    /// Activates a saved identity after validating its credential and cached profile.
    ///
    /// # Errors
    ///
    /// Returns a safe identity, credential, storage, or application-state error.
    pub fn activate_identity(
        &self,
        public_key: PublicKey,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.core.activate_identity(
            public_key,
            &self.database,
            &self.database,
            &self.database,
            secrets,
            clock,
        )
    }

    /// Signs out while retaining durable identity data and credentials.
    ///
    /// # Errors
    ///
    /// Returns a safe application-state error if sign out cannot complete.
    pub fn sign_out(&self) -> Result<AppSnapshot, SafeError> {
        self.core.sign_out()
    }

    /// Issues a revision-bound, single-use identity-removal confirmation.
    ///
    /// # Errors
    ///
    /// Returns a safe error when the target identity is not saved.
    pub fn request_identity_removal(
        &self,
        public_key: PublicKey,
        clock: &(impl Clock + ?Sized),
    ) -> Result<RemovalConfirmationToken, SafeError> {
        self.core.request_identity_removal(public_key, clock)
    }

    /// Permanently removes one confirmed identity and its credential.
    ///
    /// # Errors
    ///
    /// Returns a safe confirmation, credential, storage, recovery, or state error.
    pub fn confirm_identity_removal(
        &self,
        token: RemovalConfirmationToken,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.core.confirm_identity_removal(
            token,
            &self.database,
            &self.database,
            secrets,
            &self.database,
            clock,
        )
    }

    /// Executes a confirmed removal through the durable request coordinator.
    ///
    /// # Errors
    ///
    /// Returns a safe expiry, conflict, credential, storage, recovery, or state error.
    pub fn confirm_identity_removal_durable(
        &self,
        request_id: &DurableRequestId,
        token: RemovalConfirmationToken,
        secrets: &(impl SecretStore + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        self.core.confirm_identity_removal_durable(
            request_id,
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

    use harvestcircle_application::{
        AppLifecycle, AppStateRepository, Clock, DurableOperationKind, DurableOperationPhase,
        DurableOperationRepository, DurableRequestId, DurableTerminalOutcome, FailureSecretStore,
        IdentityOperationKind, IdentityOperationPhase, IdentityRepository, InMemorySecretStore,
        OperationJournal, OperationPriorState, RelayConfiguration, SecretStore,
        SecretStoreOperation, SessionState,
    };
    use harvestcircle_domain::{
        IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, PublicKey,
        SafeErrorCode, SecretKeyInput, SignerAvailability, UnixTimestamp,
    };
    use tempfile::tempdir;

    use super::PersistentAppCore;

    fn identity() -> NostrIdentity {
        let public_key = PublicKey::from_bytes([7; 32]).expect("valid public key");
        NostrIdentity::new(
            NostrIdentityReference::derive(public_key).expect("identity"),
            LocalKeyringBinding::new(public_key, SignerAvailability::Available),
            None,
            IdentityCreatedAt::new(UnixTimestamp::from_seconds(1).expect("time")),
            None,
        )
        .expect("identity")
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
        let path = directory
            .path()
            .canonicalize()
            .expect("canonical temporary directory")
            .join("harvestcircle.sqlite3");
        let public_key = identity().public_key();
        let secrets = InMemorySecretStore::default();
        {
            let adapter = PersistentAppCore::open(&path, RelayConfiguration::default())
                .expect("open adapter");
            let fresh = adapter
                .bootstrap(&secrets, &FixedClock)
                .expect("fresh bootstrap");
            assert!(fresh.identities().is_empty());
            adapter
                .database()
                .insert_identity(&identity())
                .expect("identity");
            adapter
                .database()
                .save_selected_identity(Some(public_key))
                .expect("selection");
        }

        let adapter =
            PersistentAppCore::open(&path, RelayConfiguration::default()).expect("reopen adapter");
        let restored = adapter.bootstrap(&secrets, &FixedClock).expect("restore");
        assert_eq!(restored.lifecycle(), AppLifecycle::Ready);
        assert_eq!(restored.identities().len(), 1);
        assert_eq!(restored.selected_identity(), Some(public_key));
        assert_eq!(restored.session(), SessionState::SignedOut);
        assert!(restored.active_identity().is_none());
    }

    #[test]
    fn corrupt_database_fails_safely_without_recreation() {
        let directory = tempdir().expect("directory");
        let path = directory
            .path()
            .canonicalize()
            .expect("canonical temporary directory")
            .join("harvestcircle.sqlite3");
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
        let path = directory
            .path()
            .canonicalize()
            .expect("canonical temporary directory")
            .join("harvestcircle.sqlite3");
        let secrets = InMemorySecretStore::default();
        let selected;
        {
            let adapter =
                PersistentAppCore::open(&path, RelayConfiguration::default()).expect("adapter");
            adapter.bootstrap(&secrets, &FixedClock).expect("bootstrap");
            let generated = adapter
                .generate_identity(&secrets, &FixedClock)
                .expect("generate");
            assert!(
                secrets
                    .contains(generated.identity().public_key())
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
            selected = imported.identity().public_key();
            assert_eq!(adapter.core().snapshot().identities().len(), 2);
        }

        let bytes = fs::read(&path).expect("database bytes");
        assert!(!bytes.windows(5).any(|value| value == b"nsec1"));
        assert!(!bytes.windows(64).any(|value| {
            value == b"7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"
        }));
        let reopened =
            PersistentAppCore::open(&path, RelayConfiguration::default()).expect("reopen");
        let restored = reopened.bootstrap(&secrets, &FixedClock).expect("restore");
        assert_eq!(restored.identities().len(), 2);
        assert_eq!(restored.selected_identity(), Some(selected));
        assert_eq!(restored.session(), SessionState::SignedOut);
    }

    #[test]
    fn durable_import_commits_each_phase_and_recovers_the_terminal_receipt() {
        let adapter = PersistentAppCore::in_memory(RelayConfiguration::default()).expect("adapter");
        let secrets = InMemorySecretStore::default();
        let snapshot = adapter.bootstrap(&secrets, &FixedClock).expect("bootstrap");
        let request =
            DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000021").expect("request");
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
        assert_eq!(receipt.identity(), imported.identity().public_key());
        assert_eq!(
            receipt.resulting_revision(),
            Some(adapter.core().snapshot().revision().value())
        );
    }

    #[test]
    fn durable_recovery_preserves_repair_metadata_and_deletes_orphan_credentials() {
        let adapter = PersistentAppCore::in_memory(RelayConfiguration::default()).expect("adapter");
        let secrets = InMemorySecretStore::default();
        let missing = identity()
            .with_local_keyring_availability(SignerAvailability::CredentialMissing)
            .expect("local keyring");
        adapter
            .database()
            .insert_identity(&missing)
            .expect("identity");
        adapter
            .database()
            .save_selected_identity(Some(missing.public_key()))
            .expect("selection");
        let request =
            DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000022").expect("request");
        adapter
            .database()
            .begin_durable_operation(
                &request,
                DurableOperationKind::Repair,
                missing.public_key(),
                Some(0),
                OperationPriorState::new(
                    Some(missing.public_key()),
                    Some(SignerAvailability::CredentialMissing),
                ),
                FixedClock.now(),
            )
            .expect("intent");
        secrets
            .put(
                missing.public_key(),
                SecretKeyInput::parse(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_owned(),
                )
                .expect("secret"),
            )
            .expect("credential");
        adapter
            .database()
            .advance_durable_operation(
                &request,
                DurableOperationPhase::IntentRecorded,
                DurableOperationPhase::CredentialWritten,
                FixedClock.now(),
                None,
            )
            .expect("credential phase");

        adapter.bootstrap(&secrets, &FixedClock).expect("recovery");
        let repaired = adapter
            .database()
            .find_identity(missing.public_key())
            .expect("lookup")
            .expect("preserved identity");
        assert_eq!(
            repaired
                .signer_binding()
                .as_local_keyring()
                .expect("local keyring")
                .availability(),
            SignerAvailability::CredentialMissing
        );
        assert!(!secrets.contains(missing.public_key()).expect("credential"));
        assert_eq!(
            adapter
                .database()
                .load_durable_operation(&request)
                .expect("operation")
                .expect("record")
                .terminal()
                .expect("receipt")
                .outcome(),
            DurableTerminalOutcome::Failed
        );
    }

    #[test]
    fn durable_recovery_covers_response_loss_and_irreversible_removal_windows() {
        let secrets = InMemorySecretStore::default();
        let adapter = PersistentAppCore::in_memory(RelayConfiguration::default()).expect("adapter");
        let saved = identity();
        adapter
            .database()
            .insert_identity(&saved)
            .expect("identity");
        let import =
            DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000023").expect("request");
        adapter
            .database()
            .begin_durable_operation(
                &import,
                DurableOperationKind::Import,
                saved.public_key(),
                Some(0),
                OperationPriorState::new(None, None),
                FixedClock.now(),
            )
            .expect("intent");
        adapter
            .database()
            .advance_durable_operation(
                &import,
                DurableOperationPhase::IntentRecorded,
                DurableOperationPhase::CredentialWritten,
                FixedClock.now(),
                None,
            )
            .expect("credential");
        adapter
            .database()
            .advance_durable_operation(
                &import,
                DurableOperationPhase::CredentialWritten,
                DurableOperationPhase::MetadataCommitted,
                FixedClock.now(),
                None,
            )
            .expect("metadata");
        let restored = adapter
            .bootstrap(&secrets, &FixedClock)
            .expect("response recovery");
        assert_eq!(restored.selected_identity(), Some(saved.public_key()));
        assert_eq!(
            adapter
                .database()
                .load_durable_operation(&import)
                .expect("operation")
                .expect("record")
                .terminal()
                .expect("receipt")
                .outcome(),
            DurableTerminalOutcome::Completed
        );

        let removal_adapter =
            PersistentAppCore::in_memory(RelayConfiguration::default()).expect("remove adapter");
        removal_adapter
            .database()
            .insert_identity(&saved)
            .expect("remove identity");
        removal_adapter
            .database()
            .save_selected_identity(Some(saved.public_key()))
            .expect("remove selection");
        let removal =
            DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000024").expect("request");
        removal_adapter
            .database()
            .begin_durable_operation(
                &removal,
                DurableOperationKind::Remove,
                saved.public_key(),
                Some(0),
                OperationPriorState::new(None, Some(SignerAvailability::Available)),
                FixedClock.now(),
            )
            .expect("remove intent");
        removal_adapter
            .database()
            .advance_durable_operation(
                &removal,
                DurableOperationPhase::IntentRecorded,
                DurableOperationPhase::CredentialDeleted,
                FixedClock.now(),
                None,
            )
            .expect("credential deleted");
        let removed = removal_adapter
            .bootstrap(&secrets, &FixedClock)
            .expect("removal recovery");
        assert!(removed.identities().is_empty());
        assert_eq!(removed.selected_identity(), None);
    }

    #[test]
    fn bootstrap_recovery_completes_credential_deleted_removal_and_fallback() {
        let directory = tempdir().expect("directory");
        let path = directory
            .path()
            .canonicalize()
            .expect("canonical temporary directory")
            .join("harvestcircle.sqlite3");
        let secrets = InMemorySecretStore::default();
        let first;
        let removed;
        {
            let adapter =
                PersistentAppCore::open(&path, RelayConfiguration::default()).expect("adapter");
            adapter.bootstrap(&secrets, &FixedClock).expect("bootstrap");
            first = adapter
                .generate_identity(&secrets, &FixedClock)
                .expect("first")
                .identity()
                .public_key();
            removed = adapter
                .generate_identity(&secrets, &FixedClock)
                .expect("removed")
                .identity()
                .public_key();
            let operation = adapter
                .database()
                .begin_operation(IdentityOperationKind::Remove, removed, FixedClock.now())
                .expect("intent");
            secrets.delete(removed).expect("credential deletion");
            adapter
                .database()
                .update_operation(
                    operation,
                    IdentityOperationPhase::CredentialDeleted,
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
        assert_eq!(restored.identities().len(), 1);
        assert_eq!(restored.selected_identity(), Some(first));
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
                .find_identity(removed)
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
            .insert_identity(&identity())
            .expect("identity");
        adapter
            .database()
            .save_selected_identity(Some(identity().public_key()))
            .expect("selection");
        adapter
            .database()
            .begin_operation(
                IdentityOperationKind::Remove,
                identity().public_key(),
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
        assert_eq!(pending[0].phase(), IdentityOperationPhase::IntentRecorded);
    }
}
