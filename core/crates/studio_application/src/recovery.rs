use radroots_studio_domain::{PublicKey, SafeError};

use crate::{
    AccountOperationKind, AccountOperationPhase, AccountRepository, AppCore, AppStateRepository,
    Clock, DurableAccountOperation, DurableOperationKind, DurableOperationPhase,
    DurableOperationRepository, DurableTerminalOutcome, OperationJournal, SecretStore,
};

impl AppCore {
    /// Reconciles durable request operations before public state is restored.
    ///
    /// # Errors
    ///
    /// Returns a safe credential, persistence, or recovery error while retaining the operation
    /// at its last durable phase for a later retry.
    pub fn recover_durable_operations(
        &self,
        accounts: &(impl AccountRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        operations: &(impl DurableOperationRepository + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<(), SafeError> {
        for operation in operations.list_unfinished_durable_operations()? {
            match operation.kind() {
                DurableOperationKind::Create
                | DurableOperationKind::Import
                | DurableOperationKind::Repair => recover_durable_addition(
                    &operation, accounts, app_state, secrets, operations, clock,
                )?,
                DurableOperationKind::Remove => recover_durable_removal(
                    &operation, accounts, app_state, secrets, operations, clock,
                )?,
            }
        }
        Ok(())
    }

    /// Reconciles non-secret cross-resource journal entries before bootstrap.
    ///
    /// An empty journal does not access the credential store.
    ///
    /// # Errors
    ///
    /// Returns a safe credential, persistence, or recovery error while retaining
    /// the unfinished journal entry for a later retry.
    pub fn recover_pending_operations(
        &self,
        accounts: &(impl AccountRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<(), SafeError> {
        for operation in journal.list_pending_operations()? {
            match operation.kind() {
                AccountOperationKind::Remove => {
                    recover_removal(&operation, accounts, app_state, secrets, journal, clock)?;
                }
                AccountOperationKind::Add | AccountOperationKind::Import => {
                    recover_addition(&operation, accounts, secrets, journal, clock)?;
                }
            }
        }
        Ok(())
    }
}

fn recover_durable_removal(
    operation: &DurableAccountOperation,
    accounts: &(impl AccountRepository + ?Sized),
    app_state: &(impl AppStateRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    operations: &(impl DurableOperationRepository + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    let request = operation.request_id();
    let account = operation.account();
    let mut phase = operation.phase();
    if phase == DurableOperationPhase::IntentRecorded {
        if secrets.contains(account)? {
            secrets.delete(account)?;
        }
        operations.advance_durable_operation(
            request,
            phase,
            DurableOperationPhase::CredentialDeleted,
            clock.now(),
            None,
        )?;
        phase = DurableOperationPhase::CredentialDeleted;
    }
    if phase == DurableOperationPhase::CredentialDeleted {
        if accounts.find_account(account)?.is_some() {
            accounts.remove_account(account)?;
        }
        operations.advance_durable_operation(
            request,
            phase,
            DurableOperationPhase::MetadataDeleted,
            clock.now(),
            None,
        )?;
        phase = DurableOperationPhase::MetadataDeleted;
    }
    if phase == DurableOperationPhase::MetadataDeleted {
        app_state.save_selected_account(operation.prior().selected_account())?;
        operations.advance_durable_operation(
            request,
            phase,
            DurableOperationPhase::SelectionCommitted,
            clock.now(),
            None,
        )?;
        phase = DurableOperationPhase::SelectionCommitted;
    }
    if phase == DurableOperationPhase::SelectionCommitted {
        operations.finalize_durable_operation(
            request,
            phase,
            DurableTerminalOutcome::Completed,
            None,
            clock.now(),
        )?;
    }
    Ok(())
}

fn recover_durable_addition(
    operation: &DurableAccountOperation,
    accounts: &(impl AccountRepository + ?Sized),
    app_state: &(impl AppStateRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    operations: &(impl DurableOperationRepository + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    let request = operation.request_id();
    let account = operation.account();
    match operation.phase() {
        DurableOperationPhase::IntentRecorded => {
            if secrets.contains(account)? {
                secrets.delete(account)?;
            }
            operations.finalize_durable_operation(
                request,
                DurableOperationPhase::IntentRecorded,
                DurableTerminalOutcome::Failed,
                None,
                clock.now(),
            )?;
        }
        DurableOperationPhase::CredentialWritten => {
            let metadata = accounts.find_account(account)?;
            let committed = metadata.as_ref().is_some_and(|saved| {
                saved.signer().availability()
                    == radroots_studio_domain::BindingAvailability::Available
            });
            if committed {
                operations.advance_durable_operation(
                    request,
                    DurableOperationPhase::CredentialWritten,
                    DurableOperationPhase::MetadataCommitted,
                    clock.now(),
                    None,
                )?;
                finish_durable_selection(operation, app_state, operations, clock)?;
            } else {
                operations.advance_durable_operation(
                    request,
                    DurableOperationPhase::CredentialWritten,
                    DurableOperationPhase::CompensationPending,
                    clock.now(),
                    None,
                )?;
                compensate_durable_addition(
                    operation, accounts, app_state, secrets, operations, clock,
                )?;
            }
        }
        DurableOperationPhase::MetadataCommitted => {
            finish_durable_selection(operation, app_state, operations, clock)?;
        }
        DurableOperationPhase::SelectionCommitted => {
            operations.finalize_durable_operation(
                request,
                DurableOperationPhase::SelectionCommitted,
                DurableTerminalOutcome::Completed,
                None,
                clock.now(),
            )?;
        }
        DurableOperationPhase::CompensationPending => {
            compensate_durable_addition(
                operation, accounts, app_state, secrets, operations, clock,
            )?;
        }
        DurableOperationPhase::CredentialDeleted | DurableOperationPhase::MetadataDeleted => {
            operations.finalize_durable_operation(
                request,
                operation.phase(),
                DurableTerminalOutcome::Failed,
                None,
                clock.now(),
            )?;
        }
        DurableOperationPhase::Finalized => {}
    }
    Ok(())
}

fn finish_durable_selection(
    operation: &DurableAccountOperation,
    app_state: &(impl AppStateRepository + ?Sized),
    operations: &(impl DurableOperationRepository + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    app_state.save_selected_account(Some(operation.account()))?;
    operations.advance_durable_operation(
        operation.request_id(),
        DurableOperationPhase::MetadataCommitted,
        DurableOperationPhase::SelectionCommitted,
        clock.now(),
        None,
    )?;
    operations.finalize_durable_operation(
        operation.request_id(),
        DurableOperationPhase::SelectionCommitted,
        DurableTerminalOutcome::Completed,
        None,
        clock.now(),
    )?;
    Ok(())
}

fn compensate_durable_addition(
    operation: &DurableAccountOperation,
    accounts: &(impl AccountRepository + ?Sized),
    app_state: &(impl AppStateRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    operations: &(impl DurableOperationRepository + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    if secrets.contains(operation.account())? {
        secrets.delete(operation.account())?;
    }
    if let Some(availability) = operation.prior().binding_availability() {
        if let Some(previous) = accounts.find_account(operation.account())? {
            accounts.update_account(&previous.with_binding_availability(availability))?;
        }
    } else if accounts.find_account(operation.account())?.is_some() {
        accounts.remove_account(operation.account())?;
    }
    app_state.save_selected_account(operation.prior().selected_account())?;
    operations.advance_durable_operation(
        operation.request_id(),
        DurableOperationPhase::CompensationPending,
        DurableOperationPhase::CredentialDeleted,
        clock.now(),
        None,
    )?;
    operations.finalize_durable_operation(
        operation.request_id(),
        DurableOperationPhase::CredentialDeleted,
        DurableTerminalOutcome::Failed,
        None,
        clock.now(),
    )?;
    Ok(())
}

fn recover_removal(
    operation: &crate::PendingAccountOperation,
    accounts: &(impl AccountRepository + ?Sized),
    app_state: &(impl AppStateRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    journal: &(impl OperationJournal + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    let public_key = operation.subject();
    if operation.phase() == AccountOperationPhase::IntentRecorded {
        match secrets.delete(public_key) {
            Ok(()) => {}
            Err(error)
                if error.code() == radroots_studio_domain::SafeErrorCode::CredentialMissing => {}
            Err(error) => return Err(error),
        }
        journal.update_operation(
            operation.id(),
            AccountOperationPhase::CredentialDeleted,
            clock.now(),
            None,
        )?;
    }
    if matches!(
        operation.phase(),
        AccountOperationPhase::IntentRecorded | AccountOperationPhase::CredentialDeleted
    ) {
        let registry = accounts.list_accounts()?;
        let selected = removal_fallback(&registry, app_state.load_selected_account()?, public_key);
        accounts.remove_account(public_key)?;
        app_state.save_selected_account(selected)?;
        journal.update_operation(
            operation.id(),
            AccountOperationPhase::MetadataDeleted,
            clock.now(),
            None,
        )?;
    }
    journal.finalize_operation(operation.id())
}

fn recover_addition(
    operation: &crate::PendingAccountOperation,
    accounts: &(impl AccountRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    journal: &(impl OperationJournal + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    let has_metadata = accounts.find_account(operation.subject())?.is_some();
    match operation.phase() {
        AccountOperationPhase::CredentialWritten | AccountOperationPhase::CompensationPending
            if !has_metadata =>
        {
            match secrets.delete(operation.subject()) {
                Ok(()) => {}
                Err(error)
                    if error.code() == radroots_studio_domain::SafeErrorCode::CredentialMissing => {
                }
                Err(error) => return Err(error),
            }
            journal.update_operation(
                operation.id(),
                AccountOperationPhase::MetadataDeleted,
                clock.now(),
                None,
            )?;
        }
        _ => {}
    }
    journal.finalize_operation(operation.id())
}

fn removal_fallback(
    registry: &[radroots_studio_domain::AccountSummary],
    selected: Option<PublicKey>,
    removed: PublicKey,
) -> Option<PublicKey> {
    if selected != Some(removed) {
        return selected;
    }
    let index = registry
        .iter()
        .position(|account| account.public_key() == removed)?;
    registry
        .get(index + 1)
        .or_else(|| index.checked_sub(1).and_then(|before| registry.get(before)))
        .map(radroots_studio_domain::AccountSummary::public_key)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Mutex, MutexGuard};

    use radroots_studio_domain::{
        BindingAvailability, PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput,
        UnixTimestamp,
    };

    use super::*;
    use crate::{
        DurableOperationReceipt, DurableOperationStart, DurableRequestId, FailureSecretStore,
        InMemoryAccountRepository, InMemoryOperationJournal, InMemorySecretStore,
        RelayConfiguration, SecretStore, SecretStoreOperation,
    };

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(10).expect("time")
        }
    }

    pub(crate) struct TestDurableRepository {
        operation: Mutex<DurableAccountOperation>,
        return_existing: bool,
    }

    impl TestDurableRepository {
        pub(crate) fn new(operation: DurableAccountOperation) -> Self {
            Self {
                operation: Mutex::new(operation),
                return_existing: true,
            }
        }

        pub(crate) fn fresh(operation: DurableAccountOperation) -> Self {
            Self {
                operation: Mutex::new(operation),
                return_existing: false,
            }
        }

        pub(crate) fn operation(&self) -> MutexGuard<'_, DurableAccountOperation> {
            self.operation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        fn replace(
            current: &DurableAccountOperation,
            phase: DurableOperationPhase,
            diagnostic: Option<crate::OperationDiagnostic>,
            terminal: Option<DurableOperationReceipt>,
        ) -> DurableAccountOperation {
            DurableAccountOperation::new(
                current.request_id().clone(),
                current.kind(),
                current.account(),
                current.expected_revision(),
                phase,
                current.prior(),
                current.updated_at(),
                diagnostic,
                terminal,
            )
        }
    }

    impl DurableOperationRepository for TestDurableRepository {
        fn begin_durable_operation(
            &self,
            _request_id: &DurableRequestId,
            _kind: DurableOperationKind,
            _account: PublicKey,
            _expected_revision: Option<u64>,
            _prior: crate::OperationPriorState,
            _updated_at: UnixTimestamp,
        ) -> Result<DurableOperationStart, SafeError> {
            let operation = self.operation().clone();
            Ok(if self.return_existing {
                DurableOperationStart::Existing(operation)
            } else {
                DurableOperationStart::Started(operation)
            })
        }

        fn load_durable_operation(
            &self,
            request_id: &DurableRequestId,
        ) -> Result<Option<DurableAccountOperation>, SafeError> {
            if !self.return_existing {
                return Ok(None);
            }
            let operation = self.operation();
            Ok((operation.request_id() == request_id).then(|| operation.clone()))
        }

        fn advance_durable_operation(
            &self,
            request_id: &DurableRequestId,
            expected_phase: DurableOperationPhase,
            next_phase: DurableOperationPhase,
            _updated_at: UnixTimestamp,
            diagnostic: Option<crate::OperationDiagnostic>,
        ) -> Result<DurableAccountOperation, SafeError> {
            let mut operation = self.operation();
            if operation.request_id() != request_id || operation.phase() != expected_phase {
                return Err(conflict());
            }
            *operation = Self::replace(&operation, next_phase, diagnostic, None);
            Ok(operation.clone())
        }

        fn finalize_durable_operation(
            &self,
            request_id: &DurableRequestId,
            expected_phase: DurableOperationPhase,
            outcome: DurableTerminalOutcome,
            resulting_revision: Option<u64>,
            _updated_at: UnixTimestamp,
        ) -> Result<DurableOperationReceipt, SafeError> {
            let mut operation = self.operation();
            if operation.request_id() != request_id || operation.phase() != expected_phase {
                return Err(conflict());
            }
            let receipt = DurableOperationReceipt::new(
                request_id.clone(),
                operation.account(),
                outcome,
                resulting_revision,
            );
            *operation = Self::replace(
                &operation,
                DurableOperationPhase::Finalized,
                operation.diagnostic(),
                Some(receipt.clone()),
            );
            Ok(receipt)
        }

        fn list_unfinished_durable_operations(
            &self,
        ) -> Result<Vec<DurableAccountOperation>, SafeError> {
            Ok(vec![self.operation().clone()])
        }
    }

    fn conflict() -> SafeError {
        SafeError::new(
            SafeErrorCode::InvalidApplicationState,
            SafeMessage::new("The test durable operation conflicted."),
        )
    }

    fn seeded() -> (
        AppCore,
        InMemoryAccountRepository,
        InMemorySecretStore,
        InMemoryOperationJournal,
        PublicKey,
    ) {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let accounts = InMemoryAccountRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let receipt = core
            .generate_account(&accounts, &accounts, &secrets, &journal, &FixedClock)
            .expect("seed account");
        (
            core,
            accounts,
            secrets,
            journal,
            receipt.account().public_key(),
        )
    }

    pub(crate) fn operation(
        kind: DurableOperationKind,
        phase: DurableOperationPhase,
        account: PublicKey,
        prior_availability: Option<BindingAvailability>,
    ) -> DurableAccountOperation {
        DurableAccountOperation::new(
            DurableRequestId::parse(format!("{kind:?}-{phase:?}")).expect("durable request ID"),
            kind,
            account,
            Some(1),
            phase,
            crate::OperationPriorState::new(None, prior_availability),
            FixedClock.now(),
            None,
            None,
        )
    }

    fn run_durable(
        core: &AppCore,
        accounts: &InMemoryAccountRepository,
        secrets: &InMemorySecretStore,
        operation: DurableAccountOperation,
    ) -> DurableAccountOperation {
        let repository = TestDurableRepository::new(operation);
        core.recover_durable_operations(accounts, accounts, secrets, &repository, &FixedClock)
            .expect("durable recovery");
        repository.operation().clone()
    }

    #[test]
    fn durable_recovery_exercises_every_removal_phase_and_presence_branch() {
        for phase in [
            DurableOperationPhase::IntentRecorded,
            DurableOperationPhase::CredentialDeleted,
            DurableOperationPhase::MetadataDeleted,
            DurableOperationPhase::SelectionCommitted,
            DurableOperationPhase::Finalized,
        ] {
            let (core, accounts, secrets, _journal, public_key) = seeded();
            if phase != DurableOperationPhase::IntentRecorded {
                secrets.delete(public_key).expect("delete credential");
            }
            if matches!(
                phase,
                DurableOperationPhase::MetadataDeleted
                    | DurableOperationPhase::SelectionCommitted
                    | DurableOperationPhase::Finalized
            ) {
                accounts.remove_account(public_key).expect("remove account");
            }
            let recovered = run_durable(
                &core,
                &accounts,
                &secrets,
                operation(DurableOperationKind::Remove, phase, public_key, None),
            );
            assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
        }

        let (core, accounts, secrets, _journal, public_key) = seeded();
        secrets.delete(public_key).expect("delete credential");
        accounts.remove_account(public_key).expect("remove account");
        let recovered = run_durable(
            &core,
            &accounts,
            &secrets,
            operation(
                DurableOperationKind::Remove,
                DurableOperationPhase::IntentRecorded,
                public_key,
                None,
            ),
        );
        assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
    }

    #[test]
    fn durable_recovery_exercises_every_addition_phase_and_compensation_shape() {
        for phase in [
            DurableOperationPhase::IntentRecorded,
            DurableOperationPhase::CredentialWritten,
            DurableOperationPhase::MetadataCommitted,
            DurableOperationPhase::SelectionCommitted,
            DurableOperationPhase::CredentialDeleted,
            DurableOperationPhase::MetadataDeleted,
            DurableOperationPhase::Finalized,
        ] {
            let (core, accounts, secrets, _journal, public_key) = seeded();
            if phase == DurableOperationPhase::IntentRecorded {
                accounts
                    .remove_account(public_key)
                    .expect("remove metadata");
            }
            let recovered = run_durable(
                &core,
                &accounts,
                &secrets,
                operation(DurableOperationKind::Create, phase, public_key, None),
            );
            assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
        }

        for (prior, retain_metadata, retain_secret) in [
            (Some(BindingAvailability::CredentialMissing), true, true),
            (Some(BindingAvailability::CredentialMissing), false, true),
            (None, true, true),
            (None, false, false),
        ] {
            let (core, accounts, secrets, _journal, public_key) = seeded();
            if !retain_metadata {
                accounts
                    .remove_account(public_key)
                    .expect("remove metadata");
            }
            if !retain_secret {
                secrets.delete(public_key).expect("delete credential");
            }
            let recovered = run_durable(
                &core,
                &accounts,
                &secrets,
                operation(
                    DurableOperationKind::Repair,
                    DurableOperationPhase::CompensationPending,
                    public_key,
                    prior,
                ),
            );
            assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
        }

        let (core, accounts, secrets, _journal, public_key) = seeded();
        accounts
            .remove_account(public_key)
            .expect("remove metadata");
        let recovered = run_durable(
            &core,
            &accounts,
            &secrets,
            operation(
                DurableOperationKind::Import,
                DurableOperationPhase::CredentialWritten,
                public_key,
                None,
            ),
        );
        assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);

        let (core, accounts, secrets, _journal, public_key) = seeded();
        accounts
            .remove_account(public_key)
            .expect("remove metadata");
        secrets.delete(public_key).expect("delete credential");
        let recovered = run_durable(
            &core,
            &accounts,
            &secrets,
            operation(
                DurableOperationKind::Create,
                DurableOperationPhase::IntentRecorded,
                public_key,
                None,
            ),
        );
        assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
    }

    #[test]
    fn pending_recovery_exercises_removal_and_addition_presence_branches() {
        for credential_present in [true, false] {
            let (core, accounts, secrets, journal, public_key) = seeded();
            if !credential_present {
                secrets.delete(public_key).expect("delete credential");
                accounts
                    .save_selected_account(None)
                    .expect("clear selection");
            }
            journal
                .begin_operation(AccountOperationKind::Remove, public_key, FixedClock.now())
                .expect("removal intent");
            core.recover_pending_operations(&accounts, &accounts, &secrets, &journal, &FixedClock)
                .expect("removal recovery");
            assert!(journal.list_pending_operations().unwrap().is_empty());
        }

        for (kind, metadata_present, credential_present) in [
            (AccountOperationKind::Add, false, true),
            (AccountOperationKind::Import, false, false),
            (AccountOperationKind::Add, true, true),
        ] {
            let (core, accounts, secrets, journal, public_key) = seeded();
            if !metadata_present {
                accounts
                    .remove_account(public_key)
                    .expect("remove metadata");
            }
            if !credential_present {
                secrets.delete(public_key).expect("delete credential");
            }
            let id = journal
                .begin_operation(kind, public_key, FixedClock.now())
                .expect("addition intent");
            journal
                .update_operation(
                    id,
                    AccountOperationPhase::CredentialWritten,
                    FixedClock.now(),
                    None,
                )
                .expect("credential phase");
            core.recover_pending_operations(&accounts, &accounts, &secrets, &journal, &FixedClock)
                .expect("addition recovery");
            assert!(journal.list_pending_operations().unwrap().is_empty());
        }

        let (core, accounts, _secrets, journal, public_key) = seeded();
        accounts
            .remove_account(public_key)
            .expect("remove metadata");
        let secrets = FailureSecretStore::default();
        secrets
            .put(
                public_key,
                SecretKeyInput::parse(
                    "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_owned(),
                )
                .expect("secret"),
            )
            .expect("store credential");
        secrets.fail_next(SecretStoreOperation::Delete);
        let id = journal
            .begin_operation(AccountOperationKind::Add, public_key, FixedClock.now())
            .expect("addition intent");
        journal
            .update_operation(
                id,
                AccountOperationPhase::CredentialWritten,
                FixedClock.now(),
                None,
            )
            .expect("credential phase");
        assert!(
            core.recover_pending_operations(&accounts, &accounts, &secrets, &journal, &FixedClock,)
                .is_err()
        );
    }
}
