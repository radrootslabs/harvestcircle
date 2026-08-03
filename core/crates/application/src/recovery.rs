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
                DurableOperationKind::Remove => {}
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
