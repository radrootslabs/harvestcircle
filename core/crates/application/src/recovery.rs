use radroots_studio_domain::{PublicKey, SafeError};

use crate::{
    AccountOperationKind, AccountOperationPhase, AccountRepository, AppCore, AppStateRepository,
    Clock, OperationJournal, SecretStore,
};

impl AppCore {
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
