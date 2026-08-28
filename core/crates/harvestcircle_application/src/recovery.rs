#[cfg(test)]
use harvestcircle_domain::PublicKey;
use harvestcircle_domain::SafeError;

use crate::{
    AppCore, AppStateRepository, Clock, DurableIdentityOperation, DurableOperationKind,
    DurableOperationPhase, DurableOperationRepository, DurableTerminalOutcome, IdentityRepository,
    SecretStore,
};
#[cfg(test)]
use crate::{IdentityOperationKind, IdentityOperationPhase, OperationJournal};

impl AppCore {
    /// Reconciles durable request operations before public state is restored.
    ///
    /// # Errors
    ///
    /// Returns a safe credential, persistence, or recovery error while retaining the operation
    /// at its last durable phase for a later retry.
    pub async fn recover_durable_operations(
        &self,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        operations: &(impl DurableOperationRepository + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<(), SafeError> {
        for operation in operations.list_unfinished_durable_operations().await? {
            match operation.kind() {
                DurableOperationKind::Create
                | DurableOperationKind::Import
                | DurableOperationKind::Repair => {
                    recover_durable_addition(
                        &operation, identities, app_state, secrets, operations, clock,
                    )
                    .await?
                }
                DurableOperationKind::Remove => {
                    recover_durable_removal(
                        &operation, identities, app_state, secrets, operations, clock,
                    )
                    .await?
                }
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
    #[cfg(test)]
    pub async fn recover_pending_operations(
        &self,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<(), SafeError> {
        for operation in journal.list_pending_operations().await? {
            match operation.kind() {
                IdentityOperationKind::Remove => {
                    recover_removal(&operation, identities, app_state, secrets, journal, clock)
                        .await?;
                }
                IdentityOperationKind::Add | IdentityOperationKind::Import => {
                    recover_addition(&operation, identities, secrets, journal, clock).await?;
                }
            }
        }
        Ok(())
    }
}

async fn recover_durable_removal(
    operation: &DurableIdentityOperation,
    identities: &(impl IdentityRepository + ?Sized),
    app_state: &(impl AppStateRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    operations: &(impl DurableOperationRepository + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    let request = operation.request_id();
    let identity = operation.identity();
    let mut phase = operation.phase();
    if phase == DurableOperationPhase::IntentRecorded {
        if secrets.contains(identity).await? {
            secrets.delete(identity).await?;
        }
        operations
            .advance_durable_operation(
                request,
                phase,
                DurableOperationPhase::CredentialDeleted,
                clock.now(),
                None,
            )
            .await?;
        phase = DurableOperationPhase::CredentialDeleted;
    }
    if phase == DurableOperationPhase::CredentialDeleted {
        if identities.find_identity(identity).await?.is_some() {
            identities.remove_identity(identity).await?;
        }
        operations
            .advance_durable_operation(
                request,
                phase,
                DurableOperationPhase::MetadataDeleted,
                clock.now(),
                None,
            )
            .await?;
        phase = DurableOperationPhase::MetadataDeleted;
    }
    if phase == DurableOperationPhase::MetadataDeleted {
        app_state
            .save_selected_identity(operation.prior().selected_identity())
            .await?;
        operations
            .advance_durable_operation(
                request,
                phase,
                DurableOperationPhase::SelectionCommitted,
                clock.now(),
                None,
            )
            .await?;
        phase = DurableOperationPhase::SelectionCommitted;
    }
    if phase == DurableOperationPhase::SelectionCommitted {
        operations
            .finalize_durable_operation(
                request,
                phase,
                DurableTerminalOutcome::Completed,
                None,
                clock.now(),
            )
            .await?;
    }
    Ok(())
}

async fn recover_durable_addition(
    operation: &DurableIdentityOperation,
    identities: &(impl IdentityRepository + ?Sized),
    app_state: &(impl AppStateRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    operations: &(impl DurableOperationRepository + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    let request = operation.request_id();
    let identity = operation.identity();
    match operation.phase() {
        DurableOperationPhase::IntentRecorded => {
            if secrets.contains(identity).await? {
                secrets.delete(identity).await?;
            }
            operations
                .finalize_durable_operation(
                    request,
                    DurableOperationPhase::IntentRecorded,
                    DurableTerminalOutcome::Failed,
                    None,
                    clock.now(),
                )
                .await?;
        }
        DurableOperationPhase::CredentialWritten => {
            let metadata = identities.find_identity(identity).await?;
            let committed = metadata.as_ref().is_some_and(|saved| {
                saved
                    .signer_binding()
                    .as_local_keyring()
                    .is_some_and(|binding| {
                        binding.availability()
                            == harvestcircle_domain::SignerAvailability::Available
                    })
            });
            if committed {
                operations
                    .advance_durable_operation(
                        request,
                        DurableOperationPhase::CredentialWritten,
                        DurableOperationPhase::MetadataCommitted,
                        clock.now(),
                        None,
                    )
                    .await?;
                finish_durable_selection(operation, app_state, operations, clock).await?;
            } else {
                operations
                    .advance_durable_operation(
                        request,
                        DurableOperationPhase::CredentialWritten,
                        DurableOperationPhase::CompensationPending,
                        clock.now(),
                        None,
                    )
                    .await?;
                compensate_durable_addition(
                    operation, identities, app_state, secrets, operations, clock,
                )
                .await?;
            }
        }
        DurableOperationPhase::MetadataCommitted => {
            finish_durable_selection(operation, app_state, operations, clock).await?;
        }
        DurableOperationPhase::SelectionCommitted => {
            operations
                .finalize_durable_operation(
                    request,
                    DurableOperationPhase::SelectionCommitted,
                    DurableTerminalOutcome::Completed,
                    None,
                    clock.now(),
                )
                .await?;
        }
        DurableOperationPhase::CompensationPending => {
            compensate_durable_addition(
                operation, identities, app_state, secrets, operations, clock,
            )
            .await?;
        }
        DurableOperationPhase::CredentialDeleted | DurableOperationPhase::MetadataDeleted => {
            operations
                .finalize_durable_operation(
                    request,
                    operation.phase(),
                    DurableTerminalOutcome::Failed,
                    None,
                    clock.now(),
                )
                .await?;
        }
        DurableOperationPhase::Finalized => {}
    }
    Ok(())
}

async fn finish_durable_selection(
    operation: &DurableIdentityOperation,
    app_state: &(impl AppStateRepository + ?Sized),
    operations: &(impl DurableOperationRepository + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    app_state
        .save_selected_identity(Some(operation.identity()))
        .await?;
    operations
        .advance_durable_operation(
            operation.request_id(),
            DurableOperationPhase::MetadataCommitted,
            DurableOperationPhase::SelectionCommitted,
            clock.now(),
            None,
        )
        .await?;
    operations
        .finalize_durable_operation(
            operation.request_id(),
            DurableOperationPhase::SelectionCommitted,
            DurableTerminalOutcome::Completed,
            None,
            clock.now(),
        )
        .await?;
    Ok(())
}

async fn compensate_durable_addition(
    operation: &DurableIdentityOperation,
    identities: &(impl IdentityRepository + ?Sized),
    app_state: &(impl AppStateRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    operations: &(impl DurableOperationRepository + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    if secrets.contains(operation.identity()).await? {
        secrets.delete(operation.identity()).await?;
    }
    if let Some(availability) = operation.prior().binding_availability() {
        if let Some(previous) = identities.find_identity(operation.identity()).await? {
            let restored = previous
                .with_local_keyring_availability(availability)
                .ok_or_else(recovery_required)?;
            identities.update_identity(&restored).await?;
        }
    } else if identities
        .find_identity(operation.identity())
        .await?
        .is_some()
    {
        identities.remove_identity(operation.identity()).await?;
    }
    app_state
        .save_selected_identity(operation.prior().selected_identity())
        .await?;
    operations
        .advance_durable_operation(
            operation.request_id(),
            DurableOperationPhase::CompensationPending,
            DurableOperationPhase::CredentialDeleted,
            clock.now(),
            None,
        )
        .await?;
    operations
        .finalize_durable_operation(
            operation.request_id(),
            DurableOperationPhase::CredentialDeleted,
            DurableTerminalOutcome::Failed,
            None,
            clock.now(),
        )
        .await?;
    Ok(())
}

const fn recovery_required() -> SafeError {
    SafeError::new(
        harvestcircle_domain::SafeErrorCode::PendingOperationRecoveryRequired,
        harvestcircle_domain::SafeMessage::new(
            "Identity recovery is required before this operation can continue.",
        ),
    )
}

#[cfg(test)]
async fn recover_removal(
    operation: &crate::PendingIdentityOperation,
    identities: &(impl IdentityRepository + ?Sized),
    app_state: &(impl AppStateRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    journal: &(impl OperationJournal + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    let public_key = operation.subject();
    if operation.phase() == IdentityOperationPhase::IntentRecorded {
        match secrets.delete(public_key).await {
            Ok(()) => {}
            Err(error)
                if error.code() == harvestcircle_domain::SafeErrorCode::CredentialMissing => {}
            Err(error) => return Err(error),
        }
        journal
            .update_operation(
                operation.id(),
                IdentityOperationPhase::CredentialDeleted,
                clock.now(),
                None,
            )
            .await?;
    }
    if matches!(
        operation.phase(),
        IdentityOperationPhase::IntentRecorded | IdentityOperationPhase::CredentialDeleted
    ) {
        let registry = identities.list_identities().await?;
        let selected = removal_fallback(
            &registry,
            app_state.load_selected_identity().await?,
            public_key,
        );
        identities.remove_identity(public_key).await?;
        app_state.save_selected_identity(selected).await?;
        journal
            .update_operation(
                operation.id(),
                IdentityOperationPhase::MetadataDeleted,
                clock.now(),
                None,
            )
            .await?;
    }
    journal.finalize_operation(operation.id()).await
}

#[cfg(test)]
async fn recover_addition(
    operation: &crate::PendingIdentityOperation,
    identities: &(impl IdentityRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    journal: &(impl OperationJournal + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    let has_metadata = identities
        .find_identity(operation.subject())
        .await?
        .is_some();
    match operation.phase() {
        IdentityOperationPhase::CredentialWritten | IdentityOperationPhase::CompensationPending
            if !has_metadata =>
        {
            match secrets.delete(operation.subject()).await {
                Ok(()) => {}
                Err(error)
                    if error.code() == harvestcircle_domain::SafeErrorCode::CredentialMissing => {}
                Err(error) => return Err(error),
            }
            journal
                .update_operation(
                    operation.id(),
                    IdentityOperationPhase::MetadataDeleted,
                    clock.now(),
                    None,
                )
                .await?;
        }
        _ => {}
    }
    journal.finalize_operation(operation.id()).await
}

#[cfg(test)]
fn removal_fallback(
    registry: &[harvestcircle_domain::NostrIdentity],
    selected: Option<PublicKey>,
    removed: PublicKey,
) -> Option<PublicKey> {
    if selected != Some(removed) {
        return selected;
    }
    let index = registry
        .iter()
        .position(|identity| identity.public_key() == removed)?;
    registry
        .get(index + 1)
        .or_else(|| index.checked_sub(1).and_then(|before| registry.get(before)))
        .map(harvestcircle_domain::NostrIdentity::public_key)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Mutex, MutexGuard};

    use harvestcircle_domain::{
        PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput, SignerAvailability,
        UnixTimestamp,
    };

    use super::*;
    use crate::{
        BoxFuture, DurableOperationReceipt, DurableOperationStart, DurableRequestId,
        FailureSecretStore, InMemoryIdentityRepository, InMemoryOperationJournal,
        InMemorySecretStore, RelayConfiguration, SecretStore, SecretStoreOperation,
    };

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(10).expect("time")
        }
    }

    pub(crate) struct TestDurableRepository {
        operation: Mutex<DurableIdentityOperation>,
        return_existing: bool,
    }

    impl TestDurableRepository {
        pub(crate) fn new(operation: DurableIdentityOperation) -> Self {
            Self {
                operation: Mutex::new(operation),
                return_existing: true,
            }
        }

        pub(crate) fn fresh(operation: DurableIdentityOperation) -> Self {
            Self {
                operation: Mutex::new(operation),
                return_existing: false,
            }
        }

        pub(crate) fn operation(&self) -> MutexGuard<'_, DurableIdentityOperation> {
            self.operation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        fn replace(
            current: &DurableIdentityOperation,
            phase: DurableOperationPhase,
            diagnostic: Option<crate::OperationDiagnostic>,
            terminal: Option<DurableOperationReceipt>,
        ) -> DurableIdentityOperation {
            DurableIdentityOperation::new(
                current.request_id().clone(),
                current.kind(),
                current.identity(),
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
        fn begin_durable_operation<'a>(
            &'a self,
            _request_id: &'a DurableRequestId,
            _kind: DurableOperationKind,
            _identity: PublicKey,
            _expected_revision: Option<u64>,
            _prior: crate::OperationPriorState,
            _updated_at: UnixTimestamp,
        ) -> BoxFuture<'a, Result<DurableOperationStart, SafeError>> {
            Box::pin(async move {
                let operation = self.operation().clone();
                Ok(if self.return_existing {
                    DurableOperationStart::Existing(operation)
                } else {
                    DurableOperationStart::Started(operation)
                })
            })
        }

        fn load_durable_operation<'a>(
            &'a self,
            request_id: &'a DurableRequestId,
        ) -> BoxFuture<'a, Result<Option<DurableIdentityOperation>, SafeError>> {
            Box::pin(async move {
                if !self.return_existing {
                    return Ok(None);
                }
                let operation = self.operation();
                Ok((operation.request_id() == request_id).then(|| operation.clone()))
            })
        }

        fn advance_durable_operation<'a>(
            &'a self,
            request_id: &'a DurableRequestId,
            expected_phase: DurableOperationPhase,
            next_phase: DurableOperationPhase,
            _updated_at: UnixTimestamp,
            diagnostic: Option<crate::OperationDiagnostic>,
        ) -> BoxFuture<'a, Result<DurableIdentityOperation, SafeError>> {
            Box::pin(async move {
                let mut operation = self.operation();
                if operation.request_id() != request_id || operation.phase() != expected_phase {
                    return Err(conflict());
                }
                *operation = Self::replace(&operation, next_phase, diagnostic, None);
                Ok(operation.clone())
            })
        }

        fn finalize_durable_operation<'a>(
            &'a self,
            request_id: &'a DurableRequestId,
            expected_phase: DurableOperationPhase,
            outcome: DurableTerminalOutcome,
            resulting_revision: Option<u64>,
            _updated_at: UnixTimestamp,
        ) -> BoxFuture<'a, Result<DurableOperationReceipt, SafeError>> {
            Box::pin(async move {
                let mut operation = self.operation();
                if operation.request_id() != request_id || operation.phase() != expected_phase {
                    return Err(conflict());
                }
                let receipt = DurableOperationReceipt::new(
                    request_id.clone(),
                    operation.identity(),
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
            })
        }

        fn list_unfinished_durable_operations(
            &self,
        ) -> BoxFuture<'_, Result<Vec<DurableIdentityOperation>, SafeError>> {
            Box::pin(async move { Ok(vec![self.operation().clone()]) })
        }
    }

    fn conflict() -> SafeError {
        SafeError::new(
            SafeErrorCode::InvalidApplicationState,
            SafeMessage::new("The test durable operation conflicted."),
        )
    }

    async fn seeded() -> (
        AppCore,
        InMemoryIdentityRepository,
        InMemorySecretStore,
        InMemoryOperationJournal,
        PublicKey,
    ) {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let receipt = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("seed identity");
        (
            core,
            identities,
            secrets,
            journal,
            receipt.identity().public_key(),
        )
    }

    pub(crate) fn operation(
        kind: DurableOperationKind,
        phase: DurableOperationPhase,
        identity: PublicKey,
        prior_availability: Option<SignerAvailability>,
    ) -> DurableIdentityOperation {
        DurableIdentityOperation::new(
            DurableRequestId::new_v7(),
            kind,
            identity,
            Some(1),
            phase,
            crate::OperationPriorState::new(None, prior_availability),
            FixedClock.now(),
            None,
            None,
        )
    }

    async fn run_durable(
        core: &AppCore,
        identities: &InMemoryIdentityRepository,
        secrets: &InMemorySecretStore,
        operation: DurableIdentityOperation,
    ) -> DurableIdentityOperation {
        let repository = TestDurableRepository::new(operation);
        core.recover_durable_operations(identities, identities, secrets, &repository, &FixedClock)
            .await
            .expect("durable recovery");
        repository.operation().clone()
    }

    #[tokio::test]
    async fn durable_recovery_exercises_every_removal_phase_and_presence_branch() {
        for phase in [
            DurableOperationPhase::IntentRecorded,
            DurableOperationPhase::CredentialDeleted,
            DurableOperationPhase::MetadataDeleted,
            DurableOperationPhase::SelectionCommitted,
            DurableOperationPhase::Finalized,
        ] {
            let (core, identities, secrets, _journal, public_key) = seeded().await;
            if phase != DurableOperationPhase::IntentRecorded {
                secrets.delete(public_key).await.expect("delete credential");
            }
            if matches!(
                phase,
                DurableOperationPhase::MetadataDeleted
                    | DurableOperationPhase::SelectionCommitted
                    | DurableOperationPhase::Finalized
            ) {
                identities
                    .remove_identity(public_key)
                    .await
                    .expect("remove identity");
            }
            let recovered = run_durable(
                &core,
                &identities,
                &secrets,
                operation(DurableOperationKind::Remove, phase, public_key, None),
            )
            .await;
            assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
        }

        let (core, identities, secrets, _journal, public_key) = seeded().await;
        secrets.delete(public_key).await.expect("delete credential");
        identities
            .remove_identity(public_key)
            .await
            .expect("remove identity");
        let recovered = run_durable(
            &core,
            &identities,
            &secrets,
            operation(
                DurableOperationKind::Remove,
                DurableOperationPhase::IntentRecorded,
                public_key,
                None,
            ),
        )
        .await;
        assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
    }

    #[tokio::test]
    async fn durable_recovery_exercises_every_addition_phase_and_compensation_shape() {
        for phase in [
            DurableOperationPhase::IntentRecorded,
            DurableOperationPhase::CredentialWritten,
            DurableOperationPhase::MetadataCommitted,
            DurableOperationPhase::SelectionCommitted,
            DurableOperationPhase::CredentialDeleted,
            DurableOperationPhase::MetadataDeleted,
            DurableOperationPhase::Finalized,
        ] {
            let (core, identities, secrets, _journal, public_key) = seeded().await;
            if phase == DurableOperationPhase::IntentRecorded {
                identities
                    .remove_identity(public_key)
                    .await
                    .expect("remove metadata");
            }
            let recovered = run_durable(
                &core,
                &identities,
                &secrets,
                operation(DurableOperationKind::Create, phase, public_key, None),
            )
            .await;
            assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
        }

        for (prior, retain_metadata, retain_secret) in [
            (Some(SignerAvailability::CredentialMissing), true, true),
            (Some(SignerAvailability::CredentialMissing), false, true),
            (None, true, true),
            (None, false, false),
        ] {
            let (core, identities, secrets, _journal, public_key) = seeded().await;
            if !retain_metadata {
                identities
                    .remove_identity(public_key)
                    .await
                    .expect("remove metadata");
            }
            if !retain_secret {
                secrets.delete(public_key).await.expect("delete credential");
            }
            let recovered = run_durable(
                &core,
                &identities,
                &secrets,
                operation(
                    DurableOperationKind::Repair,
                    DurableOperationPhase::CompensationPending,
                    public_key,
                    prior,
                ),
            )
            .await;
            assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
        }

        let (core, identities, secrets, _journal, public_key) = seeded().await;
        identities
            .remove_identity(public_key)
            .await
            .expect("remove metadata");
        let recovered = run_durable(
            &core,
            &identities,
            &secrets,
            operation(
                DurableOperationKind::Import,
                DurableOperationPhase::CredentialWritten,
                public_key,
                None,
            ),
        )
        .await;
        assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);

        let (core, identities, secrets, _journal, public_key) = seeded().await;
        identities
            .remove_identity(public_key)
            .await
            .expect("remove metadata");
        secrets.delete(public_key).await.expect("delete credential");
        let recovered = run_durable(
            &core,
            &identities,
            &secrets,
            operation(
                DurableOperationKind::Create,
                DurableOperationPhase::IntentRecorded,
                public_key,
                None,
            ),
        )
        .await;
        assert_eq!(recovered.phase(), DurableOperationPhase::Finalized);
    }

    #[tokio::test]
    async fn pending_recovery_exercises_removal_and_addition_presence_branches() {
        for credential_present in [true, false] {
            let (core, identities, secrets, journal, public_key) = seeded().await;
            if !credential_present {
                secrets.delete(public_key).await.expect("delete credential");
                identities
                    .save_selected_identity(None)
                    .await
                    .expect("clear selection");
            }
            journal
                .begin_operation(IdentityOperationKind::Remove, public_key, FixedClock.now())
                .await
                .expect("removal intent");
            core.recover_pending_operations(
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect("removal recovery");
            assert!(journal.list_pending_operations().await.unwrap().is_empty());
        }

        for (kind, metadata_present, credential_present) in [
            (IdentityOperationKind::Add, false, true),
            (IdentityOperationKind::Import, false, false),
            (IdentityOperationKind::Add, true, true),
        ] {
            let (core, identities, secrets, journal, public_key) = seeded().await;
            if !metadata_present {
                identities
                    .remove_identity(public_key)
                    .await
                    .expect("remove metadata");
            }
            if !credential_present {
                secrets.delete(public_key).await.expect("delete credential");
            }
            let id = journal
                .begin_operation(kind, public_key, FixedClock.now())
                .await
                .expect("addition intent");
            journal
                .update_operation(
                    id,
                    IdentityOperationPhase::CredentialWritten,
                    FixedClock.now(),
                    None,
                )
                .await
                .expect("credential phase");
            core.recover_pending_operations(
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect("addition recovery");
            assert!(journal.list_pending_operations().await.unwrap().is_empty());
        }

        let (core, identities, _secrets, journal, public_key) = seeded().await;
        identities
            .remove_identity(public_key)
            .await
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
            .await
            .expect("store credential");
        secrets.fail_next(SecretStoreOperation::Delete);
        let id = journal
            .begin_operation(IdentityOperationKind::Add, public_key, FixedClock.now())
            .await
            .expect("addition intent");
        journal
            .update_operation(
                id,
                IdentityOperationPhase::CredentialWritten,
                FixedClock.now(),
                None,
            )
            .await
            .expect("credential phase");
        assert!(
            core.recover_pending_operations(
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .is_err()
        );
    }
}
