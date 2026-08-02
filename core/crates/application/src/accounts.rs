use std::sync::{Mutex, MutexGuard};

use radroots_studio_domain::{
    AccountCreatedAt, AccountSummary, KeyAvailability, Nsec, PublicKey, SafeError, SafeErrorCode,
    SafeMessage, SecretKeyInput, SignerKind,
};
use radroots_studio_nostr::{generate_local_keypair, import_secret};

use crate::{
    AccountOperationKind, AccountOperationPhase, AccountRepository, AppCore, AppStateRepository,
    Clock, OperationDiagnostic, OperationId, OperationJournal, PendingAccountOperation,
    RemovalConfirmationToken, SecretStore, StateTransition,
};

pub struct GenerateAccountReceipt {
    account: AccountSummary,
    generated_nsec: Nsec,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportAccountReceipt {
    account: AccountSummary,
}

impl ImportAccountReceipt {
    #[must_use]
    pub const fn account(&self) -> &AccountSummary {
        &self.account
    }
}

impl GenerateAccountReceipt {
    #[must_use]
    pub const fn account(&self) -> &AccountSummary {
        &self.account
    }

    #[must_use]
    pub const fn generated_nsec(&self) -> &Nsec {
        &self.generated_nsec
    }
}

impl AppCore {
    /// Issues a single-use confirmation bound to the target and current revision.
    ///
    /// # Errors
    ///
    /// Returns a safe account or application-state error.
    pub fn request_account_removal(
        &self,
        public_key: PublicKey,
    ) -> Result<RemovalConfirmationToken, SafeError> {
        self.issue_removal_token(public_key)
    }

    /// Permanently removes a confirmed account and selects a deterministic fallback.
    ///
    /// # Errors
    ///
    /// Returns a safe confirmation, credential, persistence, recovery, or state error.
    pub fn confirm_account_removal(
        &self,
        token: RemovalConfirmationToken,
        accounts: &(impl AccountRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<crate::AppSnapshot, SafeError> {
        let public_key = self.consume_removal_token(token)?;
        let registry = accounts.list_accounts()?;
        let index = registry
            .iter()
            .position(|account| account.public_key() == public_key)
            .ok_or_else(account_not_found)?;
        let selected = if self.snapshot().selected_account() == Some(public_key) {
            registry
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|before| registry.get(before)))
                .map(AccountSummary::public_key)
        } else {
            self.snapshot().selected_account()
        };
        let operation =
            journal.begin_operation(AccountOperationKind::Remove, public_key, clock.now())?;
        let was_active = self
            .snapshot()
            .active_account()
            .is_some_and(|active| active.account().public_key() == public_key);
        if was_active {
            self.sign_out()?;
        }
        let account = &registry[index];
        match secrets.delete(public_key) {
            Ok(()) => {}
            Err(error)
                if error.code() == SafeErrorCode::CredentialMissing
                    && account.key_availability() == KeyAvailability::CredentialMissing => {}
            Err(error) => return Err(error),
        }
        journal.update_operation(
            operation,
            AccountOperationPhase::CredentialDeleted,
            clock.now(),
            None,
        )?;
        accounts.remove_account(public_key)?;
        app_state.save_selected_account(selected)?;
        journal.update_operation(
            operation,
            AccountOperationPhase::MetadataDeleted,
            clock.now(),
            None,
        )?;
        journal.finalize_operation(operation)?;
        self.apply_transition(StateTransition::ReplaceRegistryPreservingSession {
            accounts: accounts.list_accounts()?,
            selected,
        })
    }

    /// Persists and publishes a saved account selection without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe account, persistence, or application-state error.
    pub fn select_account(
        &self,
        public_key: PublicKey,
        accounts: &(impl AccountRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
    ) -> Result<crate::AppSnapshot, SafeError> {
        if accounts.find_account(public_key)?.is_none() {
            return Err(account_not_found());
        }
        app_state.save_selected_account(Some(public_key))?;
        self.apply_transition(StateTransition::Select(public_key))
    }

    /// Generates, stores, and selects one local Nostr account without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe key, credential, persistence, or application-state error.
    pub fn generate_account(
        &self,
        accounts: &(impl AccountRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<GenerateAccountReceipt, SafeError> {
        let generated = generate_local_keypair()?;
        let (public_key, npub, secret, nsec) = generated.into_parts();
        let account = AccountSummary::new(
            public_key,
            npub,
            SignerKind::LocalSecret,
            KeyAvailability::Available,
            None,
            AccountCreatedAt::new(clock.now()),
            None,
        );
        Self::persist_account_transaction(
            AccountOperationKind::Add,
            &account,
            secret,
            None,
            accounts,
            secrets,
            journal,
            clock,
        )?;
        app_state.save_selected_account(Some(public_key))?;
        let registry = accounts.list_accounts()?;
        self.apply_transition(StateTransition::ReplaceRegistry {
            accounts: registry,
            selected: Some(public_key),
        })?;
        Ok(GenerateAccountReceipt {
            account,
            generated_nsec: nsec,
        })
    }

    /// Imports, stores, and selects one local Nostr account without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe key, credential, persistence, or application-state error.
    pub fn import_secret_key(
        &self,
        input: SecretKeyInput,
        accounts: &(impl AccountRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportAccountReceipt, SafeError> {
        let imported = import_secret(input)?;
        let (public_key, npub, secret) = imported.into_parts();
        if let Some(existing) = accounts.find_account(public_key)? {
            if existing.key_availability() != KeyAvailability::CredentialMissing
                || secrets.contains(public_key)?
            {
                return Err(account_exists());
            }
            let repaired = existing.with_key_availability(KeyAvailability::Available);
            Self::persist_account_transaction(
                AccountOperationKind::Import,
                &repaired,
                secret,
                Some(&existing),
                accounts,
                secrets,
                journal,
                clock,
            )?;
            app_state.save_selected_account(Some(public_key))?;
            self.apply_transition(StateTransition::ReplaceRegistry {
                accounts: accounts.list_accounts()?,
                selected: Some(public_key),
            })?;
            return Ok(ImportAccountReceipt { account: repaired });
        }
        if secrets.contains(public_key)? {
            return Err(account_exists());
        }
        let account = AccountSummary::new(
            public_key,
            npub,
            SignerKind::LocalSecret,
            KeyAvailability::Available,
            None,
            AccountCreatedAt::new(clock.now()),
            None,
        );
        Self::persist_account_transaction(
            AccountOperationKind::Import,
            &account,
            secret,
            None,
            accounts,
            secrets,
            journal,
            clock,
        )?;
        app_state.save_selected_account(Some(public_key))?;
        self.apply_transition(StateTransition::ReplaceRegistry {
            accounts: accounts.list_accounts()?,
            selected: Some(public_key),
        })?;
        Ok(ImportAccountReceipt { account })
    }

    #[allow(clippy::too_many_arguments)]
    fn persist_account_transaction(
        kind: AccountOperationKind,
        account: &AccountSummary,
        secret: SecretKeyInput,
        previous: Option<&AccountSummary>,
        accounts: &(impl AccountRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<(), SafeError> {
        let public_key = account.public_key();
        let operation = journal.begin_operation(kind, public_key, clock.now())?;
        if let Err(error) = secrets.put(public_key, secret) {
            let _ = journal.finalize_operation(operation);
            return Err(error);
        }
        if let Err(error) = journal.update_operation(
            operation,
            AccountOperationPhase::CredentialWritten,
            clock.now(),
            None,
        ) {
            return compensate_account_write(
                operation, public_key, error, None, accounts, secrets, journal, clock,
            );
        }
        let metadata_result = previous.map_or_else(
            || accounts.insert_account(account),
            |_| accounts.update_account(account),
        );
        if let Err(error) = metadata_result {
            return compensate_account_write(
                operation, public_key, error, previous, accounts, secrets, journal, clock,
            );
        }
        journal.update_operation(
            operation,
            AccountOperationPhase::MetadataCommitted,
            clock.now(),
            None,
        )?;
        journal.finalize_operation(operation)
    }
}

#[allow(clippy::too_many_arguments)]
fn compensate_account_write(
    operation: OperationId,
    public_key: PublicKey,
    original_error: SafeError,
    previous: Option<&AccountSummary>,
    accounts: &(impl AccountRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    journal: &(impl OperationJournal + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    if let Some(previous) = previous {
        let _ = accounts.update_account(previous);
    } else {
        let _ = accounts.remove_account(public_key);
    }
    if secrets.delete(public_key).is_err() {
        let _ = journal.update_operation(
            operation,
            AccountOperationPhase::CompensationPending,
            clock.now(),
            Some(OperationDiagnostic::CompensationFailed),
        );
        return Err(recovery_required());
    }
    let _ = journal.finalize_operation(operation);
    Err(original_error)
}

#[derive(Default)]
pub struct InMemoryOperationJournal {
    state: Mutex<InMemoryJournalState>,
}

#[derive(Default)]
struct InMemoryJournalState {
    next_id: u64,
    pending: Vec<PendingAccountOperation>,
}

impl OperationJournal for InMemoryOperationJournal {
    fn begin_operation(
        &self,
        kind: AccountOperationKind,
        subject: PublicKey,
        updated_at: radroots_studio_domain::UnixTimestamp,
    ) -> Result<OperationId, SafeError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.next_id = state.next_id.checked_add(1).ok_or_else(recovery_required)?;
        let id = OperationId::from_raw(state.next_id);
        state.pending.push(PendingAccountOperation::new(
            id,
            kind,
            subject,
            AccountOperationPhase::IntentRecorded,
            updated_at,
            None,
        ));
        Ok(id)
    }

    fn update_operation(
        &self,
        id: OperationId,
        phase: AccountOperationPhase,
        updated_at: radroots_studio_domain::UnixTimestamp,
        diagnostic: Option<OperationDiagnostic>,
    ) -> Result<(), SafeError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let operation = state
            .pending
            .iter_mut()
            .find(|operation| operation.id() == id)
            .ok_or_else(recovery_required)?;
        *operation = PendingAccountOperation::new(
            id,
            operation.kind(),
            operation.subject(),
            phase,
            updated_at,
            diagnostic,
        );
        Ok(())
    }

    fn list_pending_operations(&self) -> Result<Vec<PendingAccountOperation>, SafeError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .clone())
    }

    fn finalize_operation(&self, id: OperationId) -> Result<(), SafeError> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .retain(|operation| operation.id() != id);
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryAccountRepository {
    state: Mutex<InMemoryAccountState>,
}

#[derive(Default)]
struct InMemoryAccountState {
    accounts: Vec<AccountSummary>,
    selected: Option<PublicKey>,
}

impl InMemoryAccountRepository {
    fn state(&self) -> MutexGuard<'_, InMemoryAccountState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl AccountRepository for InMemoryAccountRepository {
    fn list_accounts(&self) -> Result<Vec<AccountSummary>, SafeError> {
        Ok(self.state().accounts.clone())
    }

    fn find_account(&self, public_key: PublicKey) -> Result<Option<AccountSummary>, SafeError> {
        Ok(self
            .state()
            .accounts
            .iter()
            .find(|account| account.public_key() == public_key)
            .cloned())
    }

    fn insert_account(&self, account: &AccountSummary) -> Result<(), SafeError> {
        let mut state = self.state();
        if state
            .accounts
            .iter()
            .any(|saved| saved.public_key() == account.public_key())
        {
            return Err(account_exists());
        }
        state.accounts.push(account.clone());
        state
            .accounts
            .sort_by_key(|saved| (saved.created_at().timestamp(), saved.public_key()));
        Ok(())
    }

    fn update_account(&self, account: &AccountSummary) -> Result<(), SafeError> {
        let mut state = self.state();
        let saved = state
            .accounts
            .iter_mut()
            .find(|saved| saved.public_key() == account.public_key())
            .ok_or_else(account_not_found)?;
        *saved = account.clone();
        Ok(())
    }

    fn remove_account(&self, public_key: PublicKey) -> Result<(), SafeError> {
        let mut state = self.state();
        state
            .accounts
            .retain(|account| account.public_key() != public_key);
        if state.selected == Some(public_key) {
            state.selected = None;
        }
        Ok(())
    }
}

impl AppStateRepository for InMemoryAccountRepository {
    fn load_selected_account(&self) -> Result<Option<PublicKey>, SafeError> {
        Ok(self.state().selected)
    }

    fn save_selected_account(&self, public_key: Option<PublicKey>) -> Result<(), SafeError> {
        let mut state = self.state();
        if public_key.is_some_and(|key| {
            !state
                .accounts
                .iter()
                .any(|account| account.public_key() == key)
        }) {
            return Err(account_not_found());
        }
        state.selected = public_key;
        Ok(())
    }
}

const fn account_exists() -> SafeError {
    SafeError::new(
        SafeErrorCode::AccountAlreadyExists,
        SafeMessage::new("The Nostr account is already saved."),
    )
}

const fn account_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::AccountNotFound,
        SafeMessage::new("The account was not found."),
    )
}

const fn recovery_required() -> SafeError {
    SafeError::new(
        SafeErrorCode::PendingOperationRecoveryRequired,
        SafeMessage::new("Account recovery is required before this operation can continue."),
    )
}

#[cfg(test)]
mod tests {
    use radroots_studio_domain::{
        AccountCreatedAt, AccountSummary, KeyAvailability, PublicKey, SafeError, SafeErrorCode,
        SafeMessage, SecretKeyInput, SignerKind, UnixTimestamp,
    };

    use super::InMemoryAccountRepository;
    use crate::{
        AccountOperationPhase, AccountRepository, AppCore, AppStateRepository, Clock,
        FailureSecretStore, InMemoryOperationJournal, InMemorySecretStore, OperationJournal,
        RelayConfiguration, SecretStore, SecretStoreOperation, SessionState, StateTransition,
    };

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(10).expect("time")
        }
    }

    #[derive(Default)]
    struct FailingInsertRepository {
        inner: InMemoryAccountRepository,
    }

    impl AccountRepository for FailingInsertRepository {
        fn list_accounts(&self) -> Result<Vec<AccountSummary>, SafeError> {
            self.inner.list_accounts()
        }

        fn find_account(&self, public_key: PublicKey) -> Result<Option<AccountSummary>, SafeError> {
            self.inner.find_account(public_key)
        }

        fn insert_account(&self, _account: &AccountSummary) -> Result<(), SafeError> {
            Err(SafeError::new(
                SafeErrorCode::StorageUnavailable,
                SafeMessage::new("The test account repository is unavailable."),
            ))
        }

        fn update_account(&self, account: &AccountSummary) -> Result<(), SafeError> {
            self.inner.update_account(account)
        }

        fn remove_account(&self, public_key: PublicKey) -> Result<(), SafeError> {
            self.inner.remove_account(public_key)
        }
    }

    impl AppStateRepository for FailingInsertRepository {
        fn load_selected_account(&self) -> Result<Option<PublicKey>, SafeError> {
            self.inner.load_selected_account()
        }

        fn save_selected_account(&self, public_key: Option<PublicKey>) -> Result<(), SafeError> {
            self.inner.save_selected_account(public_key)
        }
    }

    #[test]
    fn generate_account_stores_selects_and_returns_one_time_nsec_without_activation() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");

        let receipt = core
            .generate_account(&accounts, &accounts, &secrets, &journal, &FixedClock)
            .expect("generate");
        let public_key = receipt.account().public_key();
        assert_eq!(public_key.to_hex().len(), 64);
        assert!(secrets.contains(public_key).expect("credential"));
        assert_eq!(
            accounts.load_selected_account().expect("selection"),
            Some(public_key)
        );
        assert_eq!(core.snapshot().selected_account(), Some(public_key));
        assert_eq!(core.snapshot().session(), SessionState::SignedOut);
        assert!(core.snapshot().active_account().is_none());
        assert_eq!(receipt.generated_nsec().with_exposed_secret(str::len), 63);
        assert!(!format!("{:?}", core.snapshot()).contains("nsec1"));
    }

    #[test]
    fn import_secret_key_accepts_nsec_and_hex_without_exposing_or_activating() {
        for input in [
            "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5",
            "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
        ] {
            let core = AppCore::in_memory(RelayConfiguration::default());
            let accounts = InMemoryAccountRepository::default();
            let secrets = InMemorySecretStore::default();
            let journal = InMemoryOperationJournal::default();
            core.bootstrap().expect("bootstrap");
            let receipt = core
                .import_secret_key(
                    SecretKeyInput::parse(input.to_owned()).expect("input"),
                    &accounts,
                    &accounts,
                    &secrets,
                    &journal,
                    &FixedClock,
                )
                .expect("import");
            let public_key = receipt.account().public_key();
            assert!(secrets.contains(public_key).expect("credential"));
            assert_eq!(core.snapshot().selected_account(), Some(public_key));
            assert_eq!(core.snapshot().session(), SessionState::SignedOut);
            assert!(!format!("{:?}", core.snapshot()).contains(input));
        }
    }

    #[test]
    fn import_secret_key_rejects_invalid_nsec_checksum_before_persistence() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let input = SecretKeyInput::parse(
            "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_owned(),
        )
        .expect("domain shape");
        let error = core
            .import_secret_key(input, &accounts, &accounts, &secrets, &journal, &FixedClock)
            .expect_err("invalid import");
        assert_eq!(error.code(), SafeErrorCode::InvalidSecretKey);
        assert!(core.snapshot().accounts().is_empty());
    }

    #[test]
    fn duplicate_import_preserves_existing_credential_and_snapshot() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let import = || {
            SecretKeyInput::parse(
                "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_owned(),
            )
            .expect("input")
        };
        core.import_secret_key(
            import(),
            &accounts,
            &accounts,
            &secrets,
            &journal,
            &FixedClock,
        )
        .expect("first import");
        let before = core.snapshot();
        let error = core
            .import_secret_key(
                import(),
                &accounts,
                &accounts,
                &secrets,
                &journal,
                &FixedClock,
            )
            .expect_err("duplicate");
        assert_eq!(error.code(), SafeErrorCode::AccountAlreadyExists);
        assert_eq!(core.snapshot(), before);
        assert_eq!(core.snapshot().accounts().len(), 1);
    }

    #[test]
    fn duplicate_import_repairs_only_explicit_missing_credential_account() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let input = || {
            SecretKeyInput::parse(
                "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_owned(),
            )
            .expect("input")
        };
        let imported = radroots_studio_nostr::import_secret(input()).expect("derive");
        let (public_key, npub, _) = imported.into_parts();
        let missing = AccountSummary::new(
            public_key,
            npub,
            SignerKind::LocalSecret,
            KeyAvailability::CredentialMissing,
            None,
            AccountCreatedAt::new(FixedClock.now()),
            None,
        );
        accounts.insert_account(&missing).expect("missing metadata");
        accounts
            .save_selected_account(Some(public_key))
            .expect("selection");
        core.apply_transition(StateTransition::ReplaceRegistry {
            accounts: vec![missing],
            selected: Some(public_key),
        })
        .expect("registry");

        let receipt = core
            .import_secret_key(
                input(),
                &accounts,
                &accounts,
                &secrets,
                &journal,
                &FixedClock,
            )
            .expect("repair");
        assert_eq!(
            receipt.account().key_availability(),
            KeyAvailability::Available
        );
        assert!(secrets.contains(public_key).expect("credential"));
        assert_eq!(core.snapshot().accounts().len(), 1);
    }

    #[test]
    fn account_transaction_publishes_nothing_when_credential_write_fails() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = FailureSecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        secrets.fail_next(SecretStoreOperation::Put);

        let error = core
            .generate_account(&accounts, &accounts, &secrets, &journal, &FixedClock)
            .err()
            .expect("credential failure");
        assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
        assert!(core.snapshot().accounts().is_empty());
        assert!(
            journal
                .list_pending_operations()
                .expect("journal")
                .is_empty()
        );
    }

    #[test]
    fn account_transaction_removes_written_credential_when_metadata_fails() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = FailingInsertRepository::default();
        let secrets = FailureSecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");

        let error = core
            .generate_account(&accounts, &accounts, &secrets, &journal, &FixedClock)
            .err()
            .expect("metadata failure");
        assert_eq!(error.code(), SafeErrorCode::StorageUnavailable);
        let calls = secrets.calls();
        assert_eq!(calls[0].operation(), SecretStoreOperation::Put);
        assert_eq!(calls[1].operation(), SecretStoreOperation::Delete);
        assert_eq!(calls[0].public_key(), calls[1].public_key());
        assert!(core.snapshot().accounts().is_empty());
        assert!(
            journal
                .list_pending_operations()
                .expect("journal")
                .is_empty()
        );
    }

    #[test]
    fn account_transaction_retains_non_secret_journal_when_compensation_fails() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = FailingInsertRepository::default();
        let secrets = FailureSecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        secrets.fail_next(SecretStoreOperation::Delete);

        let error = core
            .generate_account(&accounts, &accounts, &secrets, &journal, &FixedClock)
            .err()
            .expect("recovery required");
        assert_eq!(
            error.code(),
            SafeErrorCode::PendingOperationRecoveryRequired
        );
        let pending = journal.list_pending_operations().expect("journal");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].phase(),
            AccountOperationPhase::CompensationPending
        );
        assert!(!format!("{pending:?}").contains("nsec1"));
        assert!(core.snapshot().accounts().is_empty());
    }

    #[test]
    fn select_account_persists_existing_choice_without_activating() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let first = core
            .generate_account(&accounts, &accounts, &secrets, &journal, &FixedClock)
            .expect("first")
            .account()
            .public_key();
        core.generate_account(&accounts, &accounts, &secrets, &journal, &FixedClock)
            .expect("second");

        let selected = core
            .select_account(first, &accounts, &accounts)
            .expect("select first");
        assert_eq!(selected.selected_account(), Some(first));
        assert_eq!(selected.session(), SessionState::SignedOut);
        assert!(selected.active_account().is_none());
        assert_eq!(
            accounts.load_selected_account().expect("saved"),
            Some(first)
        );
        let missing = core
            .select_account(PublicKey::from_bytes([0xff; 32]), &accounts, &accounts)
            .expect_err("missing account");
        assert_eq!(missing.code(), SafeErrorCode::AccountNotFound);
        assert_eq!(core.snapshot(), selected);
    }

    #[test]
    fn remove_account_requires_fresh_single_use_confirmation_and_selects_next_fallback() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let first = core
            .generate_account(&accounts, &accounts, &secrets, &journal, &FixedClock)
            .expect("first")
            .account()
            .public_key();
        let second = core
            .generate_account(&accounts, &accounts, &secrets, &journal, &FixedClock)
            .expect("second")
            .account()
            .public_key();
        core.select_account(first, &accounts, &accounts)
            .expect("select first");
        let stale = core.request_account_removal(first).expect("stale token");
        core.select_account(second, &accounts, &accounts)
            .expect("change revision");
        let stale_error = core
            .confirm_account_removal(stale, &accounts, &accounts, &secrets, &journal, &FixedClock)
            .expect_err("stale token");
        assert_eq!(stale_error.code(), SafeErrorCode::InvalidApplicationState);
        assert_eq!(core.snapshot().accounts().len(), 2);

        core.select_account(first, &accounts, &accounts)
            .expect("reselect first");
        let token = core.request_account_removal(first).expect("token");
        let removed = core
            .confirm_account_removal(token, &accounts, &accounts, &secrets, &journal, &FixedClock)
            .expect("remove");
        assert_eq!(removed.accounts().len(), 1);
        assert_eq!(removed.selected_account(), Some(second));
        assert!(!secrets.contains(first).expect("credential removed"));
        assert_eq!(removed.session(), SessionState::SignedOut);
    }
}
