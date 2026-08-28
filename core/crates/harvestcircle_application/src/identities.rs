use std::sync::{Mutex, MutexGuard};

use crate::{
    AppCore, AppStateRepository, BoxFuture, Clock, DurableOperationKind, DurableOperationPhase,
    DurableOperationRepository, DurableOperationStart, DurableRequestId, DurableTerminalOutcome,
    IdentityRepository, OperationPriorState, RemovalConfirmationToken, SecretStore,
    StagedGeneratedKey, StateTransition,
};
#[cfg(test)]
use crate::{
    IdentityOperationKind, IdentityOperationPhase, OperationDiagnostic, OperationId,
    OperationJournal, PendingIdentityOperation,
};
use harvestcircle_domain::{
    IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, Nsec, PublicKey,
    SafeError, SafeErrorCode, SafeMessage, SecretKeyInput, SignerAvailability,
};

pub struct GenerateIdentityReceipt {
    identity: NostrIdentity,
    generated_nsec: Nsec,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportIdentityReceipt {
    identity: NostrIdentity,
}

impl ImportIdentityReceipt {
    #[must_use]
    pub const fn identity(&self) -> &NostrIdentity {
        &self.identity
    }
}

impl GenerateIdentityReceipt {
    #[must_use]
    pub const fn identity(&self) -> &NostrIdentity {
        &self.identity
    }

    #[must_use]
    pub const fn generated_nsec(&self) -> &Nsec {
        &self.generated_nsec
    }
}

impl AppCore {
    /// Commits a staged generated key only after its recovery acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict, keyring, persistence, or recovery error.
    #[allow(clippy::too_many_arguments)]
    pub async fn commit_staged_generated_key(
        &self,
        request_id: &DurableRequestId,
        staged: StagedGeneratedKey,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        operations: &(impl DurableOperationRepository + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportIdentityReceipt, SafeError> {
        let expected_revision = staged.expected_revision();
        self.require_revision(expected_revision)?;
        let (identity, secret) = staged.into_commit_parts();
        self.persist_identity_durable(
            request_id,
            DurableOperationKind::Create,
            expected_revision,
            &identity,
            secret,
            None,
            identities,
            app_state,
            secrets,
            operations,
            clock,
        )
        .await?;
        Ok(ImportIdentityReceipt { identity })
    }

    /// Generates and commits one identity under a durable caller request.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict, keyring, persistence, or state error. Staged recovery transport
    /// replaces this transitional generated-secret receipt in the custody phase.
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_identity_durable(
        &self,
        request_id: &DurableRequestId,
        expected_revision: u64,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        operations: &(impl DurableOperationRepository + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<GenerateIdentityReceipt, SafeError> {
        self.require_revision(expected_revision)?;
        let generated = self.key_material().generate()?;
        let (public_key, npub, secret, nsec) = generated.into_parts();
        let identity = NostrIdentity::new(
            NostrIdentityReference::verify(public_key, npub.as_str().to_owned())?,
            LocalKeyringBinding::new(public_key, SignerAvailability::Available),
            None,
            IdentityCreatedAt::new(clock.now()),
            None,
        )?;
        self.persist_identity_durable(
            request_id,
            DurableOperationKind::Create,
            expected_revision,
            &identity,
            secret,
            None,
            identities,
            app_state,
            secrets,
            operations,
            clock,
        )
        .await?;
        Ok(GenerateIdentityReceipt {
            identity,
            generated_nsec: nsec,
        })
    }

    /// Imports or explicitly repairs one local identity under a durable caller request.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict, validation, keyring, persistence, or state error.
    #[allow(clippy::too_many_arguments)]
    pub async fn import_secret_key_durable(
        &self,
        request_id: &DurableRequestId,
        expected_revision: u64,
        input: SecretKeyInput,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        operations: &(impl DurableOperationRepository + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportIdentityReceipt, SafeError> {
        if let Some(existing) = operations.load_durable_operation(request_id).await? {
            return if existing
                .terminal()
                .is_some_and(|receipt| receipt.outcome() == DurableTerminalOutcome::Completed)
            {
                identities
                    .find_identity(existing.identity())
                    .await?
                    .map(|identity| ImportIdentityReceipt { identity })
                    .ok_or_else(recovery_required)
            } else {
                Err(recovery_required())
            };
        }
        self.require_revision(expected_revision)?;
        let imported = self.key_material().import(input)?;
        let (public_key, npub, secret) = imported.into_parts();
        let previous = identities.find_identity(public_key).await?;
        if let Some(existing) = &previous
            && (local_keyring_binding(existing)?.availability()
                != SignerAvailability::CredentialMissing
                || secrets.contains(public_key).await?)
        {
            return Err(identity_exists());
        }
        if previous.is_none() && secrets.contains(public_key).await? {
            return Err(identity_exists());
        }
        let identity = if let Some(existing) = &previous {
            existing
                .with_local_keyring_availability(SignerAvailability::Available)
                .ok_or_else(recovery_required)?
        } else {
            NostrIdentity::new(
                NostrIdentityReference::verify(public_key, npub.as_str().to_owned())?,
                LocalKeyringBinding::new(public_key, SignerAvailability::Available),
                None,
                IdentityCreatedAt::new(clock.now()),
                None,
            )?
        };
        let kind = if previous.is_some() {
            DurableOperationKind::Repair
        } else {
            DurableOperationKind::Import
        };
        self.persist_identity_durable(
            request_id,
            kind,
            expected_revision,
            &identity,
            secret,
            previous.as_ref(),
            identities,
            app_state,
            secrets,
            operations,
            clock,
        )
        .await?;
        Ok(ImportIdentityReceipt { identity })
    }

    fn require_revision(&self, expected_revision: u64) -> Result<(), SafeError> {
        if self.snapshot().revision().value() != expected_revision {
            return Err(operation_conflict());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn persist_identity_durable(
        &self,
        request_id: &DurableRequestId,
        kind: DurableOperationKind,
        expected_revision: u64,
        identity: &NostrIdentity,
        secret: SecretKeyInput,
        previous: Option<&NostrIdentity>,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        operations: &(impl DurableOperationRepository + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<(), SafeError> {
        let prior_availability = previous
            .map(local_keyring_binding)
            .transpose()?
            .map(LocalKeyringBinding::availability);
        let prior = OperationPriorState::new(
            app_state.load_selected_identity().await?,
            prior_availability,
        );
        match operations
            .begin_durable_operation(
                request_id,
                kind,
                identity.public_key(),
                Some(expected_revision),
                prior,
                clock.now(),
            )
            .await?
        {
            DurableOperationStart::Started(_) => {}
            DurableOperationStart::Existing(operation) => {
                return if operation
                    .terminal()
                    .is_some_and(|receipt| receipt.outcome() == DurableTerminalOutcome::Completed)
                {
                    Ok(())
                } else {
                    Err(recovery_required())
                };
            }
        }
        secrets.put(identity.public_key(), secret).await?;
        operations
            .advance_durable_operation(
                request_id,
                DurableOperationPhase::IntentRecorded,
                DurableOperationPhase::CredentialWritten,
                clock.now(),
                None,
            )
            .await?;
        if previous.is_some() {
            identities.update_identity(identity).await?;
        } else {
            identities.insert_identity(identity).await?;
        }
        operations
            .advance_durable_operation(
                request_id,
                DurableOperationPhase::CredentialWritten,
                DurableOperationPhase::MetadataCommitted,
                clock.now(),
                None,
            )
            .await?;
        app_state
            .save_selected_identity(Some(identity.public_key()))
            .await?;
        operations
            .advance_durable_operation(
                request_id,
                DurableOperationPhase::MetadataCommitted,
                DurableOperationPhase::SelectionCommitted,
                clock.now(),
                None,
            )
            .await?;
        let snapshot = self.apply_transition(StateTransition::ReplaceRegistry {
            identities: identities.list_identities().await?,
            selected: Some(identity.public_key()),
        })?;
        operations
            .finalize_durable_operation(
                request_id,
                DurableOperationPhase::SelectionCommitted,
                DurableTerminalOutcome::Completed,
                Some(snapshot.revision().value()),
                clock.now(),
            )
            .await?;
        Ok(())
    }

    /// Issues a single-use confirmation bound to the target and current revision.
    ///
    /// # Errors
    ///
    /// Returns a safe identity or application-state error.
    pub fn request_identity_removal(
        &self,
        public_key: PublicKey,
        clock: &(impl Clock + ?Sized),
    ) -> Result<RemovalConfirmationToken, SafeError> {
        self.issue_removal_token(public_key, clock.now())
    }

    pub fn cancel_identity_removal(&self, token: RemovalConfirmationToken) -> bool {
        self.cancel_removal_token(token)
    }

    /// Permanently removes a confirmed identity and selects a deterministic fallback.
    ///
    /// # Errors
    ///
    /// Returns a safe confirmation, credential, persistence, recovery, or state error.
    #[cfg(test)]
    pub async fn confirm_identity_removal(
        &self,
        token: RemovalConfirmationToken,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<crate::AppSnapshot, SafeError> {
        let public_key = self.consume_removal_token(token, clock.now())?;
        let registry = identities.list_identities().await?;
        let index = registry
            .iter()
            .position(|identity| identity.public_key() == public_key)
            .ok_or_else(identity_not_found)?;
        let selected = if self.snapshot().selected_identity() == Some(public_key) {
            registry
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|before| registry.get(before)))
                .map(NostrIdentity::public_key)
        } else {
            self.snapshot().selected_identity()
        };
        let operation = journal
            .begin_operation(IdentityOperationKind::Remove, public_key, clock.now())
            .await?;
        let was_active = self
            .snapshot()
            .active_identity()
            .is_some_and(|active| active.identity().public_key() == public_key);
        if was_active {
            self.sign_out()?;
        }
        let identity = &registry[index];
        let local_keyring = local_keyring_binding(identity)?;
        match secrets.delete(public_key).await {
            Ok(()) => {}
            Err(error)
                if error.code() == SafeErrorCode::CredentialMissing
                    && local_keyring.availability() == SignerAvailability::CredentialMissing => {}
            Err(error) => return Err(error),
        }
        journal
            .update_operation(
                operation,
                IdentityOperationPhase::CredentialDeleted,
                clock.now(),
                None,
            )
            .await?;
        identities.remove_identity(public_key).await?;
        app_state.save_selected_identity(selected).await?;
        journal
            .update_operation(
                operation,
                IdentityOperationPhase::MetadataDeleted,
                clock.now(),
                None,
            )
            .await?;
        journal.finalize_operation(operation).await?;
        self.apply_transition(StateTransition::ReplaceRegistryPreservingSession {
            identities: identities.list_identities().await?,
            selected,
        })
    }

    /// Confirms and executes an expiring removal plan as a durable request.
    ///
    /// # Errors
    ///
    /// Returns a safe expiry, conflict, credential, persistence, or recovery error.
    #[allow(clippy::too_many_arguments)]
    pub async fn confirm_identity_removal_durable(
        &self,
        request_id: &DurableRequestId,
        token: RemovalConfirmationToken,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        operations: &(impl DurableOperationRepository + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<crate::AppSnapshot, SafeError> {
        let expected_revision = token.revision().value();
        let public_key = self.consume_removal_token(token, clock.now())?;
        self.require_revision(expected_revision)?;
        let registry = identities.list_identities().await?;
        let index = registry
            .iter()
            .position(|identity| identity.public_key() == public_key)
            .ok_or_else(identity_not_found)?;
        let selected = if self.snapshot().selected_identity() == Some(public_key) {
            registry
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|before| registry.get(before)))
                .map(NostrIdentity::public_key)
        } else {
            self.snapshot().selected_identity()
        };
        let identity = &registry[index];
        let local_keyring = local_keyring_binding(identity)?;
        match operations
            .begin_durable_operation(
                request_id,
                DurableOperationKind::Remove,
                public_key,
                Some(expected_revision),
                OperationPriorState::new(selected, Some(local_keyring.availability())),
                clock.now(),
            )
            .await?
        {
            DurableOperationStart::Started(_) => {}
            DurableOperationStart::Existing(operation) => {
                return if operation
                    .terminal()
                    .is_some_and(|receipt| receipt.outcome() == DurableTerminalOutcome::Completed)
                {
                    Ok(self.snapshot())
                } else {
                    Err(recovery_required())
                };
            }
        }
        if self
            .snapshot()
            .active_identity()
            .is_some_and(|active| active.identity().public_key() == public_key)
        {
            self.sign_out()?;
        }
        match secrets.delete(public_key).await {
            Ok(()) => {}
            Err(error)
                if error.code() == SafeErrorCode::CredentialMissing
                    && local_keyring.availability() == SignerAvailability::CredentialMissing => {}
            Err(error) => return Err(error),
        }
        operations
            .advance_durable_operation(
                request_id,
                DurableOperationPhase::IntentRecorded,
                DurableOperationPhase::CredentialDeleted,
                clock.now(),
                None,
            )
            .await?;
        identities.remove_identity(public_key).await?;
        operations
            .advance_durable_operation(
                request_id,
                DurableOperationPhase::CredentialDeleted,
                DurableOperationPhase::MetadataDeleted,
                clock.now(),
                None,
            )
            .await?;
        app_state.save_selected_identity(selected).await?;
        operations
            .advance_durable_operation(
                request_id,
                DurableOperationPhase::MetadataDeleted,
                DurableOperationPhase::SelectionCommitted,
                clock.now(),
                None,
            )
            .await?;
        let snapshot =
            self.apply_transition(StateTransition::ReplaceRegistryPreservingSession {
                identities: identities.list_identities().await?,
                selected,
            })?;
        operations
            .finalize_durable_operation(
                request_id,
                DurableOperationPhase::SelectionCommitted,
                DurableTerminalOutcome::Completed,
                Some(snapshot.revision().value()),
                clock.now(),
            )
            .await?;
        Ok(snapshot)
    }

    /// Persists and publishes a saved identity selection without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe identity, persistence, or application-state error.
    pub async fn select_identity(
        &self,
        public_key: PublicKey,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
    ) -> Result<crate::AppSnapshot, SafeError> {
        if identities.find_identity(public_key).await?.is_none() {
            return Err(identity_not_found());
        }
        app_state.save_selected_identity(Some(public_key)).await?;
        self.apply_transition(StateTransition::Select(public_key))
    }

    /// Generates, stores, and selects one local Nostr identity without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe key, credential, persistence, or application-state error.
    #[cfg(test)]
    pub async fn generate_identity(
        &self,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<GenerateIdentityReceipt, SafeError> {
        let generated = self.key_material().generate()?;
        let (public_key, npub, secret, nsec) = generated.into_parts();
        let identity = NostrIdentity::new(
            NostrIdentityReference::verify(public_key, npub.as_str().to_owned())?,
            LocalKeyringBinding::new(public_key, SignerAvailability::Available),
            None,
            IdentityCreatedAt::new(clock.now()),
            None,
        )?;
        Self::persist_identity_transaction(
            IdentityOperationKind::Add,
            &identity,
            secret,
            None,
            identities,
            app_state,
            secrets,
            journal,
            clock,
        )
        .await?;
        let registry = identities.list_identities().await?;
        self.apply_transition(StateTransition::ReplaceRegistry {
            identities: registry,
            selected: Some(public_key),
        })?;
        Ok(GenerateIdentityReceipt {
            identity,
            generated_nsec: nsec,
        })
    }

    /// Imports, stores, and selects one local Nostr identity without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe key, credential, persistence, or application-state error.
    #[cfg(test)]
    pub async fn import_secret_key(
        &self,
        input: SecretKeyInput,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<ImportIdentityReceipt, SafeError> {
        let imported = self.key_material().import(input)?;
        let (public_key, npub, secret) = imported.into_parts();
        if let Some(existing) = identities.find_identity(public_key).await? {
            if local_keyring_binding(&existing)?.availability()
                != SignerAvailability::CredentialMissing
                || secrets.contains(public_key).await?
            {
                return Err(identity_exists());
            }
            let repaired = existing
                .with_local_keyring_availability(SignerAvailability::Available)
                .ok_or_else(recovery_required)?;
            Self::persist_identity_transaction(
                IdentityOperationKind::Import,
                &repaired,
                secret,
                Some(&existing),
                identities,
                app_state,
                secrets,
                journal,
                clock,
            )
            .await?;
            self.apply_transition(StateTransition::ReplaceRegistry {
                identities: identities.list_identities().await?,
                selected: Some(public_key),
            })?;
            return Ok(ImportIdentityReceipt { identity: repaired });
        }
        if secrets.contains(public_key).await? {
            return Err(identity_exists());
        }
        let identity = NostrIdentity::new(
            NostrIdentityReference::verify(public_key, npub.as_str().to_owned())?,
            LocalKeyringBinding::new(public_key, SignerAvailability::Available),
            None,
            IdentityCreatedAt::new(clock.now()),
            None,
        )?;
        Self::persist_identity_transaction(
            IdentityOperationKind::Import,
            &identity,
            secret,
            None,
            identities,
            app_state,
            secrets,
            journal,
            clock,
        )
        .await?;
        self.apply_transition(StateTransition::ReplaceRegistry {
            identities: identities.list_identities().await?,
            selected: Some(public_key),
        })?;
        Ok(ImportIdentityReceipt { identity })
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    async fn persist_identity_transaction(
        kind: IdentityOperationKind,
        identity: &NostrIdentity,
        secret: SecretKeyInput,
        previous: Option<&NostrIdentity>,
        identities: &(impl IdentityRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
        secrets: &(impl SecretStore + ?Sized),
        journal: &(impl OperationJournal + ?Sized),
        clock: &(impl Clock + ?Sized),
    ) -> Result<(), SafeError> {
        let public_key = identity.public_key();
        let previous_selection = app_state.load_selected_identity().await?;
        let operation = journal
            .begin_operation(kind, public_key, clock.now())
            .await?;
        if let Err(error) = secrets.put(public_key, secret).await {
            let _ = journal.finalize_operation(operation).await;
            return Err(error);
        }
        if let Err(error) = journal
            .update_operation(
                operation,
                IdentityOperationPhase::CredentialWritten,
                clock.now(),
                None,
            )
            .await
        {
            return compensate_identity_write(
                operation,
                public_key,
                error,
                None,
                previous_selection,
                identities,
                app_state,
                secrets,
                journal,
                clock,
            )
            .await;
        }
        let metadata_result = if previous.is_some() {
            identities.update_identity(identity).await
        } else {
            identities.insert_identity(identity).await
        };
        if let Err(error) = metadata_result {
            return compensate_identity_write(
                operation,
                public_key,
                error,
                previous,
                previous_selection,
                identities,
                app_state,
                secrets,
                journal,
                clock,
            )
            .await;
        }
        if let Err(error) = app_state.save_selected_identity(Some(public_key)).await {
            return compensate_identity_write(
                operation,
                public_key,
                error,
                previous,
                previous_selection,
                identities,
                app_state,
                secrets,
                journal,
                clock,
            )
            .await;
        }
        journal
            .update_operation(
                operation,
                IdentityOperationPhase::MetadataCommitted,
                clock.now(),
                None,
            )
            .await?;
        journal.finalize_operation(operation).await
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
async fn compensate_identity_write(
    operation: OperationId,
    public_key: PublicKey,
    original_error: SafeError,
    previous: Option<&NostrIdentity>,
    previous_selection: Option<PublicKey>,
    identities: &(impl IdentityRepository + ?Sized),
    app_state: &(impl AppStateRepository + ?Sized),
    secrets: &(impl SecretStore + ?Sized),
    journal: &(impl OperationJournal + ?Sized),
    clock: &(impl Clock + ?Sized),
) -> Result<(), SafeError> {
    let metadata_rollback = if let Some(previous) = previous {
        identities.update_identity(previous).await
    } else {
        identities.remove_identity(public_key).await
    };
    let selection_rollback = app_state.save_selected_identity(previous_selection).await;
    let credential_rollback = secrets.delete(public_key).await;
    if metadata_rollback.is_err() || selection_rollback.is_err() || credential_rollback.is_err() {
        let _ = journal
            .update_operation(
                operation,
                IdentityOperationPhase::CompensationPending,
                clock.now(),
                Some(OperationDiagnostic::CompensationFailed),
            )
            .await;
        return Err(recovery_required());
    }
    let _ = journal.finalize_operation(operation).await;
    Err(original_error)
}

#[cfg(test)]
#[derive(Default)]
pub struct InMemoryOperationJournal {
    state: Mutex<InMemoryJournalState>,
}

#[cfg(test)]
#[derive(Default)]
struct InMemoryJournalState {
    next_id: u64,
    pending: Vec<PendingIdentityOperation>,
}

#[cfg(test)]
impl OperationJournal for InMemoryOperationJournal {
    fn begin_operation<'a>(
        &'a self,
        kind: IdentityOperationKind,
        subject: PublicKey,
        updated_at: harvestcircle_domain::UnixTimestamp,
    ) -> BoxFuture<'a, Result<OperationId, SafeError>> {
        Box::pin(async move {
            let mut state = self.state.lock().map_err(|_| recovery_required())?;
            state.next_id = state.next_id.checked_add(1).ok_or_else(recovery_required)?;
            let id = OperationId::from_raw(state.next_id);
            state.pending.push(PendingIdentityOperation::new(
                id,
                kind,
                subject,
                IdentityOperationPhase::IntentRecorded,
                updated_at,
                None,
            ));
            Ok(id)
        })
    }

    fn update_operation<'a>(
        &'a self,
        id: OperationId,
        phase: IdentityOperationPhase,
        updated_at: harvestcircle_domain::UnixTimestamp,
        diagnostic: Option<OperationDiagnostic>,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let mut state = self.state.lock().map_err(|_| recovery_required())?;
            let operation = state
                .pending
                .iter_mut()
                .find(|operation| operation.id() == id)
                .ok_or_else(recovery_required)?;
            *operation = PendingIdentityOperation::new(
                id,
                operation.kind(),
                operation.subject(),
                phase,
                updated_at,
                diagnostic,
            );
            Ok(())
        })
    }

    fn list_pending_operations(
        &self,
    ) -> BoxFuture<'_, Result<Vec<PendingIdentityOperation>, SafeError>> {
        Box::pin(async move {
            Ok(self
                .state
                .lock()
                .map_err(|_| recovery_required())?
                .pending
                .clone())
        })
    }

    fn finalize_operation(&self, id: OperationId) -> BoxFuture<'_, Result<(), SafeError>> {
        Box::pin(async move {
            self.state
                .lock()
                .map_err(|_| recovery_required())?
                .pending
                .retain(|operation| operation.id() != id);
            Ok(())
        })
    }
}

#[derive(Default)]
pub struct InMemoryIdentityRepository {
    state: Mutex<InMemoryIdentityState>,
}

#[derive(Default)]
struct InMemoryIdentityState {
    identities: Vec<NostrIdentity>,
    selected: Option<PublicKey>,
}

impl InMemoryIdentityRepository {
    fn state(&self) -> Result<MutexGuard<'_, InMemoryIdentityState>, SafeError> {
        self.state.lock().map_err(|_| recovery_required())
    }
}

impl IdentityRepository for InMemoryIdentityRepository {
    fn list_identities(&self) -> BoxFuture<'_, Result<Vec<NostrIdentity>, SafeError>> {
        Box::pin(async move { Ok(self.state()?.identities.clone()) })
    }

    fn find_identity(
        &self,
        public_key: PublicKey,
    ) -> BoxFuture<'_, Result<Option<NostrIdentity>, SafeError>> {
        Box::pin(async move {
            Ok(self
                .state()?
                .identities
                .iter()
                .find(|identity| identity.public_key() == public_key)
                .cloned())
        })
    }

    fn insert_identity<'a>(
        &'a self,
        identity: &'a NostrIdentity,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let mut state = self.state()?;
            if state
                .identities
                .iter()
                .any(|saved| saved.public_key() == identity.public_key())
            {
                return Err(identity_exists());
            }
            state.identities.push(identity.clone());
            state
                .identities
                .sort_by_key(|saved| (saved.created_at().timestamp(), saved.public_key()));
            Ok(())
        })
    }

    fn update_identity<'a>(
        &'a self,
        identity: &'a NostrIdentity,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let mut state = self.state()?;
            let saved = state
                .identities
                .iter_mut()
                .find(|saved| saved.public_key() == identity.public_key())
                .ok_or_else(identity_not_found)?;
            *saved = identity.clone();
            Ok(())
        })
    }

    fn remove_identity(&self, public_key: PublicKey) -> BoxFuture<'_, Result<(), SafeError>> {
        Box::pin(async move {
            let mut state = self.state()?;
            state
                .identities
                .retain(|identity| identity.public_key() != public_key);
            if state.selected == Some(public_key) {
                state.selected = None;
            }
            Ok(())
        })
    }
}

impl AppStateRepository for InMemoryIdentityRepository {
    fn load_selected_identity(&self) -> BoxFuture<'_, Result<Option<PublicKey>, SafeError>> {
        Box::pin(async move { Ok(self.state()?.selected) })
    }

    fn save_selected_identity(
        &self,
        public_key: Option<PublicKey>,
    ) -> BoxFuture<'_, Result<(), SafeError>> {
        Box::pin(async move {
            let mut state = self.state()?;
            if public_key.is_some_and(|key| {
                !state
                    .identities
                    .iter()
                    .any(|identity| identity.public_key() == key)
            }) {
                return Err(identity_not_found());
            }
            state.selected = public_key;
            Ok(())
        })
    }
}

const fn identity_exists() -> SafeError {
    SafeError::new(
        SafeErrorCode::IdentityAlreadyExists,
        SafeMessage::new("The Nostr identity is already saved."),
    )
}

const fn identity_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::IdentityNotFound,
        SafeMessage::new("The identity was not found."),
    )
}

fn local_keyring_binding(identity: &NostrIdentity) -> Result<LocalKeyringBinding, SafeError> {
    identity
        .signer_binding()
        .as_local_keyring()
        .ok_or_else(recovery_required)
}

const fn recovery_required() -> SafeError {
    SafeError::new(
        SafeErrorCode::PendingOperationRecoveryRequired,
        SafeMessage::new("Identity recovery is required before this operation can continue."),
    )
}

const fn operation_conflict() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The identity operation conflicts with the current application state."),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use harvestcircle_domain::{
        IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, PublicKey,
        SafeError, SafeErrorCode, SafeMessage, SecretKeyInput, SignerAvailability, UnixTimestamp,
    };

    use super::InMemoryIdentityRepository;
    use crate::{
        AppCore, AppStateRepository, BoxFuture, Clock, DurableOperationKind, DurableOperationPhase,
        FailureSecretStore, IdentityOperationPhase, IdentityRepository, InMemoryOperationJournal,
        InMemorySecretStore, OperationJournal, ProfileRefreshStatus, ProfileRepository,
        RelayConfiguration, SecretStore, SecretStoreOperation, SessionState, StateTransition,
        recovery::tests::{TestDurableRepository, operation as durable_operation},
    };

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(10).expect("time")
        }
    }

    struct LateClock;

    impl Clock for LateClock {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(311).expect("time")
        }
    }

    struct EmptyProfiles;

    impl ProfileRepository for EmptyProfiles {
        fn load_profile(
            &self,
            _public_key: PublicKey,
        ) -> BoxFuture<'_, Result<Option<crate::CachedProfile>, SafeError>> {
            Box::pin(async { Ok(None) })
        }

        fn save_profile<'a>(
            &'a self,
            _profile: &'a crate::CachedProfile,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            Box::pin(async { Ok(()) })
        }

        fn record_refresh_status<'a>(
            &'a self,
            _public_key: PublicKey,
            _refreshed_at: UnixTimestamp,
            _status: ProfileRefreshStatus,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            Box::pin(async { Ok(()) })
        }

        fn remove_profile(&self, _public_key: PublicKey) -> BoxFuture<'_, Result<(), SafeError>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Default)]
    struct FailingUpdateJournal(InMemoryOperationJournal);

    impl OperationJournal for FailingUpdateJournal {
        fn begin_operation<'a>(
            &'a self,
            kind: crate::IdentityOperationKind,
            subject: PublicKey,
            updated_at: UnixTimestamp,
        ) -> BoxFuture<'a, Result<crate::OperationId, SafeError>> {
            self.0.begin_operation(kind, subject, updated_at)
        }

        fn update_operation<'a>(
            &'a self,
            _id: crate::OperationId,
            _phase: IdentityOperationPhase,
            _updated_at: UnixTimestamp,
            _diagnostic: Option<crate::OperationDiagnostic>,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            Box::pin(async {
                Err(SafeError::new(
                    SafeErrorCode::StorageUnavailable,
                    SafeMessage::new("The test journal is unavailable."),
                ))
            })
        }

        fn list_pending_operations(
            &self,
        ) -> BoxFuture<'_, Result<Vec<crate::PendingIdentityOperation>, SafeError>> {
            self.0.list_pending_operations()
        }

        fn finalize_operation(
            &self,
            id: crate::OperationId,
        ) -> BoxFuture<'_, Result<(), SafeError>> {
            self.0.finalize_operation(id)
        }
    }

    #[derive(Default)]
    struct FailingInsertRepository {
        inner: InMemoryIdentityRepository,
    }

    #[derive(Default)]
    struct FailingSelectionRepository {
        inner: InMemoryIdentityRepository,
        fail_next_selection: AtomicBool,
    }

    impl IdentityRepository for FailingSelectionRepository {
        fn list_identities(&self) -> BoxFuture<'_, Result<Vec<NostrIdentity>, SafeError>> {
            self.inner.list_identities()
        }

        fn find_identity(
            &self,
            public_key: PublicKey,
        ) -> BoxFuture<'_, Result<Option<NostrIdentity>, SafeError>> {
            self.inner.find_identity(public_key)
        }

        fn insert_identity<'a>(
            &'a self,
            identity: &'a NostrIdentity,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            self.inner.insert_identity(identity)
        }

        fn update_identity<'a>(
            &'a self,
            identity: &'a NostrIdentity,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            self.inner.update_identity(identity)
        }

        fn remove_identity(&self, public_key: PublicKey) -> BoxFuture<'_, Result<(), SafeError>> {
            self.inner.remove_identity(public_key)
        }
    }

    impl AppStateRepository for FailingSelectionRepository {
        fn load_selected_identity(&self) -> BoxFuture<'_, Result<Option<PublicKey>, SafeError>> {
            self.inner.load_selected_identity()
        }

        fn save_selected_identity(
            &self,
            public_key: Option<PublicKey>,
        ) -> BoxFuture<'_, Result<(), SafeError>> {
            if self.fail_next_selection.swap(false, Ordering::SeqCst) {
                return Box::pin(async {
                    Err(SafeError::new(
                        SafeErrorCode::StorageUnavailable,
                        SafeMessage::new("The test selection repository is unavailable."),
                    ))
                });
            }
            self.inner.save_selected_identity(public_key)
        }
    }

    impl IdentityRepository for FailingInsertRepository {
        fn list_identities(&self) -> BoxFuture<'_, Result<Vec<NostrIdentity>, SafeError>> {
            self.inner.list_identities()
        }

        fn find_identity(
            &self,
            public_key: PublicKey,
        ) -> BoxFuture<'_, Result<Option<NostrIdentity>, SafeError>> {
            self.inner.find_identity(public_key)
        }

        fn insert_identity<'a>(
            &'a self,
            _identity: &'a NostrIdentity,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            Box::pin(async {
                Err(SafeError::new(
                    SafeErrorCode::StorageUnavailable,
                    SafeMessage::new("The test identity repository is unavailable."),
                ))
            })
        }

        fn update_identity<'a>(
            &'a self,
            identity: &'a NostrIdentity,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            self.inner.update_identity(identity)
        }

        fn remove_identity(&self, public_key: PublicKey) -> BoxFuture<'_, Result<(), SafeError>> {
            self.inner.remove_identity(public_key)
        }
    }

    impl AppStateRepository for FailingInsertRepository {
        fn load_selected_identity(&self) -> BoxFuture<'_, Result<Option<PublicKey>, SafeError>> {
            self.inner.load_selected_identity()
        }

        fn save_selected_identity(
            &self,
            public_key: Option<PublicKey>,
        ) -> BoxFuture<'_, Result<(), SafeError>> {
            self.inner.save_selected_identity(public_key)
        }
    }

    #[tokio::test]
    async fn generate_identity_stores_selects_and_returns_one_time_nsec_without_activation() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");

        let receipt = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("generate");
        let public_key = receipt.identity().public_key();
        assert_eq!(public_key.to_hex().len(), 64);
        assert!(secrets.contains(public_key).await.expect("credential"));
        assert_eq!(
            identities
                .load_selected_identity()
                .await
                .expect("selection"),
            Some(public_key)
        );
        assert_eq!(core.snapshot().selected_identity(), Some(public_key));
        assert_eq!(core.snapshot().session(), SessionState::SignedOut);
        assert!(core.snapshot().active_identity().is_none());
        assert_eq!(receipt.generated_nsec().with_exposed_secret(str::len), 63);
        assert!(!format!("{:?}", core.snapshot()).contains("nsec1"));
    }

    #[tokio::test]
    async fn import_secret_key_accepts_nsec_and_hex_without_exposing_or_activating() {
        for input in [
            "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5",
            "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7",
        ] {
            let core = AppCore::in_memory(RelayConfiguration::default());
            let identities = InMemoryIdentityRepository::default();
            let secrets = InMemorySecretStore::default();
            let journal = InMemoryOperationJournal::default();
            core.bootstrap().expect("bootstrap");
            let receipt = core
                .import_secret_key(
                    SecretKeyInput::parse(input.to_owned()).expect("input"),
                    &identities,
                    &identities,
                    &secrets,
                    &journal,
                    &FixedClock,
                )
                .await
                .expect("import");
            let public_key = receipt.identity().public_key();
            assert!(secrets.contains(public_key).await.expect("credential"));
            assert_eq!(core.snapshot().selected_identity(), Some(public_key));
            assert_eq!(core.snapshot().session(), SessionState::SignedOut);
            assert!(!format!("{:?}", core.snapshot()).contains(input));
        }
    }

    #[tokio::test]
    async fn import_secret_key_rejects_invalid_nsec_checksum_before_persistence() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let input = SecretKeyInput::parse(
            "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_owned(),
        )
        .expect("domain shape");
        let error = core
            .import_secret_key(
                input,
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect_err("invalid import");
        assert_eq!(error.code(), SafeErrorCode::InvalidSecretKey);
        assert!(core.snapshot().identities().is_empty());
    }

    #[tokio::test]
    async fn duplicate_import_preserves_existing_credential_and_snapshot() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
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
            &identities,
            &identities,
            &secrets,
            &journal,
            &FixedClock,
        )
        .await
        .expect("first import");
        let before = core.snapshot();
        let error = core
            .import_secret_key(
                import(),
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect_err("duplicate");
        assert_eq!(error.code(), SafeErrorCode::IdentityAlreadyExists);
        assert_eq!(core.snapshot(), before);
        assert_eq!(core.snapshot().identities().len(), 1);
    }

    #[tokio::test]
    async fn duplicate_import_repairs_only_explicit_missing_credential_identity() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let input = || {
            SecretKeyInput::parse(
                "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_owned(),
            )
            .expect("input")
        };
        let imported = core.key_material().import(input()).expect("derive");
        let (public_key, npub, _) = imported.into_parts();
        let missing = NostrIdentity::new(
            NostrIdentityReference::verify(public_key, npub.as_str().to_owned()).expect("identity"),
            LocalKeyringBinding::new(public_key, SignerAvailability::CredentialMissing),
            None,
            IdentityCreatedAt::new(FixedClock.now()),
            None,
        )
        .expect("missing identity");
        identities
            .insert_identity(&missing)
            .await
            .expect("missing metadata");
        identities
            .save_selected_identity(Some(public_key))
            .await
            .expect("selection");
        core.apply_transition(StateTransition::ReplaceRegistry {
            identities: vec![missing],
            selected: Some(public_key),
        })
        .expect("registry");

        let receipt = core
            .import_secret_key(
                input(),
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect("repair");
        assert_eq!(
            receipt
                .identity()
                .signer_binding()
                .as_local_keyring()
                .expect("local keyring")
                .availability(),
            SignerAvailability::Available
        );
        assert!(secrets.contains(public_key).await.expect("credential"));
        assert_eq!(core.snapshot().identities().len(), 1);
    }

    #[tokio::test]
    async fn identity_transaction_publishes_nothing_when_credential_write_fails() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = FailureSecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        secrets.fail_next(SecretStoreOperation::Put);

        let error = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .err()
            .expect("credential failure");
        assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
        assert!(core.snapshot().identities().is_empty());
        assert!(
            journal
                .list_pending_operations()
                .await
                .expect("journal")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn identity_transaction_removes_written_credential_when_metadata_fails() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = FailingInsertRepository::default();
        let secrets = FailureSecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");

        let error = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .err()
            .expect("metadata failure");
        assert_eq!(error.code(), SafeErrorCode::StorageUnavailable);
        let calls = secrets.calls();
        assert_eq!(calls[0].operation(), SecretStoreOperation::Put);
        assert_eq!(calls[1].operation(), SecretStoreOperation::Delete);
        assert_eq!(calls[0].public_key(), calls[1].public_key());
        assert!(core.snapshot().identities().is_empty());
        assert!(
            journal
                .list_pending_operations()
                .await
                .expect("journal")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn identity_transaction_rolls_back_metadata_and_credential_when_selection_fails() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = FailingSelectionRepository::default();
        let secrets = FailureSecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        identities.fail_next_selection.store(true, Ordering::SeqCst);

        let error = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .err()
            .expect("selection failure");

        assert_eq!(error.code(), SafeErrorCode::StorageUnavailable);
        assert!(
            identities
                .list_identities()
                .await
                .expect("identities")
                .is_empty()
        );
        assert_eq!(
            identities
                .load_selected_identity()
                .await
                .expect("selection"),
            None
        );
        let calls = secrets.calls();
        assert_eq!(calls[0].operation(), SecretStoreOperation::Put);
        assert_eq!(calls[1].operation(), SecretStoreOperation::Delete);
        assert_eq!(calls[0].public_key(), calls[1].public_key());
        assert!(core.snapshot().identities().is_empty());
        assert!(
            journal
                .list_pending_operations()
                .await
                .expect("journal")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn identity_transaction_retains_non_secret_journal_when_compensation_fails() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = FailingInsertRepository::default();
        let secrets = FailureSecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        secrets.fail_next(SecretStoreOperation::Delete);

        let error = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .err()
            .expect("recovery required");
        assert_eq!(
            error.code(),
            SafeErrorCode::PendingOperationRecoveryRequired
        );
        let pending = journal.list_pending_operations().await.expect("journal");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].phase(),
            IdentityOperationPhase::CompensationPending
        );
        assert!(!format!("{pending:?}").contains("nsec1"));
        assert!(core.snapshot().identities().is_empty());
    }

    #[tokio::test]
    async fn select_identity_persists_existing_choice_without_activating() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let first = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("first")
            .identity()
            .public_key();
        core.generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("second");

        let selected = core
            .select_identity(first, &identities, &identities)
            .await
            .expect("select first");
        assert_eq!(selected.selected_identity(), Some(first));
        assert_eq!(selected.session(), SessionState::SignedOut);
        assert!(selected.active_identity().is_none());
        assert_eq!(
            identities.load_selected_identity().await.expect("saved"),
            Some(first)
        );
        let missing = core
            .select_identity(
                PublicKey::from_hex(
                    "e0266e3cfb0d2886f91c73f5f868f3b98273713e5fcd97c081663f5518a4b3af",
                )
                .expect("unknown public key"),
                &identities,
                &identities,
            )
            .await
            .expect_err("missing identity");
        assert_eq!(missing.code(), SafeErrorCode::IdentityNotFound);
        assert_eq!(core.snapshot(), selected);
    }

    #[tokio::test]
    async fn remove_identity_requires_fresh_single_use_confirmation_and_selects_next_fallback() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let first = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("first")
            .identity()
            .public_key();
        let second = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("second")
            .identity()
            .public_key();
        core.select_identity(first, &identities, &identities)
            .await
            .expect("select first");
        let stale = core
            .request_identity_removal(first, &FixedClock)
            .expect("stale token");
        core.select_identity(second, &identities, &identities)
            .await
            .expect("change revision");
        let stale_error = core
            .confirm_identity_removal(
                stale,
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect_err("stale token");
        assert_eq!(stale_error.code(), SafeErrorCode::InvalidApplicationState);
        assert_eq!(core.snapshot().identities().len(), 2);

        core.select_identity(first, &identities, &identities)
            .await
            .expect("reselect first");
        let token = core
            .request_identity_removal(first, &FixedClock)
            .expect("token");
        let removed = core
            .confirm_identity_removal(
                token,
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect("remove");
        assert_eq!(removed.identities().len(), 1);
        assert_eq!(removed.selected_identity(), Some(second));
        assert!(!secrets.contains(first).await.expect("credential removed"));
        assert_eq!(removed.session(), SessionState::SignedOut);
    }

    #[tokio::test]
    async fn removal_preflight_reports_impact_expires_and_can_be_cancelled() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let identity = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("identity")
            .identity()
            .public_key();
        let expired = core
            .request_identity_removal(identity, &FixedClock)
            .expect("plan");
        assert!(expired.impact().deletes_local_credential());
        assert!(!expired.impact().signs_out());
        assert!(
            core.confirm_identity_removal(
                expired,
                &identities,
                &identities,
                &secrets,
                &journal,
                &LateClock,
            )
            .await
            .is_err()
        );
        let cancelled = core
            .request_identity_removal(identity, &FixedClock)
            .expect("replacement plan");
        assert!(core.cancel_identity_removal(cancelled));
        assert_eq!(core.snapshot().identities().len(), 1);
    }

    #[tokio::test]
    async fn import_rejects_orphan_credentials_and_durable_nonterminal_replays() {
        const SECRET: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let material = core
            .key_material()
            .import(SecretKeyInput::parse(SECRET.to_owned()).expect("secret"))
            .expect("key material");
        let (public_key, _npub, secret) = material.into_parts();
        secrets
            .put(public_key, secret)
            .await
            .expect("orphan credential");

        assert_eq!(
            core.import_secret_key(
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect_err("orphan credential must fail")
            .code(),
            SafeErrorCode::IdentityAlreadyExists
        );

        let pending = durable_operation(
            DurableOperationKind::Import,
            DurableOperationPhase::IntentRecorded,
            public_key,
            None,
        );
        let request_id = pending.request_id().clone();
        let operations = TestDurableRepository::new(pending);
        assert_eq!(
            core.import_secret_key_durable(
                &request_id,
                core.snapshot().revision().value(),
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
                &identities,
                &identities,
                &secrets,
                &operations,
                &FixedClock,
            )
            .await
            .expect_err("unfinished replay must require recovery")
            .code(),
            SafeErrorCode::PendingOperationRecoveryRequired
        );
    }

    #[tokio::test]
    async fn durable_import_covers_new_and_missing_credential_repair_paths() {
        const SECRET: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
        for repair in [false, true] {
            let core = AppCore::in_memory(RelayConfiguration::default());
            let identities = InMemoryIdentityRepository::default();
            let secrets = InMemorySecretStore::default();
            let material = core
                .key_material()
                .import(SecretKeyInput::parse(SECRET.to_owned()).expect("secret"))
                .expect("key material");
            let (public_key, npub, secret) = material.into_parts();
            drop(secret);
            if repair {
                let identity = NostrIdentity::new(
                    NostrIdentityReference::verify(public_key, npub.as_str().to_owned())
                        .expect("identity"),
                    LocalKeyringBinding::new(public_key, SignerAvailability::CredentialMissing),
                    None,
                    IdentityCreatedAt::new(FixedClock.now()),
                    None,
                )
                .expect("identity");
                identities
                    .insert_identity(&identity)
                    .await
                    .expect("insert identity");
                identities
                    .save_selected_identity(Some(public_key))
                    .await
                    .expect("selection");
                core.apply_transition(StateTransition::BootstrapRegistry {
                    identities: vec![identity],
                    selected: Some(public_key),
                })
                .expect("registry");
            } else {
                core.bootstrap().expect("bootstrap");
            }
            let kind = if repair {
                DurableOperationKind::Repair
            } else {
                DurableOperationKind::Import
            };
            let pending = durable_operation(
                kind,
                DurableOperationPhase::IntentRecorded,
                public_key,
                repair.then_some(SignerAvailability::CredentialMissing),
            );
            let request_id = pending.request_id().clone();
            let operations = TestDurableRepository::fresh(pending);
            let receipt = core
                .import_secret_key_durable(
                    &request_id,
                    core.snapshot().revision().value(),
                    SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
                    &identities,
                    &identities,
                    &secrets,
                    &operations,
                    &FixedClock,
                )
                .await
                .expect("durable import");
            assert_eq!(receipt.identity().public_key(), public_key);
            assert_eq!(
                operations.operation().phase(),
                DurableOperationPhase::Finalized
            );
        }
    }

    #[tokio::test]
    async fn removal_of_unselected_identity_preserves_the_current_selection() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        core.bootstrap().expect("bootstrap");
        let first = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("first")
            .identity()
            .public_key();
        let second = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("second")
            .identity()
            .public_key();
        let token = core
            .request_identity_removal(first, &FixedClock)
            .expect("removal token");
        let snapshot = core
            .confirm_identity_removal(
                token,
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect("remove unselected identity");
        assert_eq!(snapshot.selected_identity(), Some(second));

        let missing = crate::test_support::valid_test_public_key(99).expect("missing key");
        assert!(
            identities
                .insert_identity(&snapshot.identities()[0])
                .await
                .is_err()
        );
        assert!(
            identities
                .save_selected_identity(Some(missing))
                .await
                .is_err()
        );

        let third = core
            .generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
            .await
            .expect("third")
            .identity()
            .public_key();
        let token = core
            .request_identity_removal(second, &FixedClock)
            .expect("durable removal token");
        let pending = durable_operation(
            DurableOperationKind::Remove,
            DurableOperationPhase::IntentRecorded,
            second,
            Some(SignerAvailability::Available),
        );
        let request_id = pending.request_id().clone();
        let operations = TestDurableRepository::fresh(pending);
        let snapshot = core
            .confirm_identity_removal_durable(
                &request_id,
                token,
                &identities,
                &identities,
                &secrets,
                &operations,
                &FixedClock,
            )
            .await
            .expect("durable unselected removal");
        assert_eq!(snapshot.selected_identity(), Some(third));
    }

    #[tokio::test]
    async fn duplicate_missing_binding_with_orphan_credential_fails_closed() {
        const SECRET: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = InMemoryOperationJournal::default();
        let material = core
            .key_material()
            .import(SecretKeyInput::parse(SECRET.to_owned()).expect("secret"))
            .expect("key material");
        let (public_key, npub, secret) = material.into_parts();
        let identity = NostrIdentity::new(
            NostrIdentityReference::verify(public_key, npub.as_str().to_owned()).expect("identity"),
            LocalKeyringBinding::new(public_key, SignerAvailability::CredentialMissing),
            None,
            IdentityCreatedAt::new(FixedClock.now()),
            None,
        )
        .expect("identity");
        identities
            .insert_identity(&identity)
            .await
            .expect("insert identity");
        identities
            .save_selected_identity(Some(public_key))
            .await
            .expect("selection");
        secrets.put(public_key, secret).await.expect("credential");
        core.apply_transition(StateTransition::BootstrapRegistry {
            identities: vec![identity],
            selected: Some(public_key),
        })
        .expect("registry");

        assert_eq!(
            core.import_secret_key(
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
                &identities,
                &identities,
                &secrets,
                &journal,
                &FixedClock,
            )
            .await
            .expect_err("orphan credential must fail")
            .code(),
            SafeErrorCode::IdentityAlreadyExists
        );
        let pending = durable_operation(
            DurableOperationKind::Repair,
            DurableOperationPhase::IntentRecorded,
            public_key,
            Some(SignerAvailability::CredentialMissing),
        );
        let request_id = pending.request_id().clone();
        let operations = TestDurableRepository::fresh(pending);
        assert_eq!(
            core.import_secret_key_durable(
                &request_id,
                core.snapshot().revision().value(),
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
                &identities,
                &identities,
                &secrets,
                &operations,
                &FixedClock,
            )
            .await
            .expect_err("orphan durable credential must fail")
            .code(),
            SafeErrorCode::IdentityAlreadyExists
        );
    }

    #[tokio::test]
    async fn removing_an_active_identity_signs_out_for_legacy_and_durable_requests() {
        for durable in [false, true] {
            let core = AppCore::in_memory(RelayConfiguration::default());
            let identities = InMemoryIdentityRepository::default();
            let secrets = InMemorySecretStore::default();
            let journal = InMemoryOperationJournal::default();
            core.bootstrap().expect("bootstrap");
            let public_key = core
                .import_secret_key(
                    SecretKeyInput::parse(
                        "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"
                            .to_owned(),
                    )
                    .expect("secret"),
                    &identities,
                    &identities,
                    &secrets,
                    &journal,
                    &FixedClock,
                )
                .await
                .expect("identity")
                .identity()
                .public_key();
            core.activate_identity(
                public_key,
                &identities,
                &identities,
                &EmptyProfiles,
                &secrets,
                &FixedClock,
            )
            .await
            .expect("activate identity");
            let token = core
                .request_identity_removal(public_key, &FixedClock)
                .expect("removal token");
            let snapshot = if durable {
                let pending = durable_operation(
                    DurableOperationKind::Remove,
                    DurableOperationPhase::IntentRecorded,
                    public_key,
                    Some(SignerAvailability::Available),
                );
                let request_id = pending.request_id().clone();
                let operations = TestDurableRepository::fresh(pending);
                core.confirm_identity_removal_durable(
                    &request_id,
                    token,
                    &identities,
                    &identities,
                    &secrets,
                    &operations,
                    &FixedClock,
                )
                .await
                .expect("durable removal")
            } else {
                core.confirm_identity_removal(
                    token,
                    &identities,
                    &identities,
                    &secrets,
                    &journal,
                    &FixedClock,
                )
                .await
                .expect("removal")
            };
            assert_eq!(snapshot.session(), SessionState::SignedOut);
        }
    }

    #[tokio::test]
    async fn identity_transaction_compensates_a_journal_phase_failure() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let identities = InMemoryIdentityRepository::default();
        let secrets = InMemorySecretStore::default();
        let journal = FailingUpdateJournal::default();
        core.bootstrap().expect("bootstrap");

        assert_eq!(
            core.generate_identity(&identities, &identities, &secrets, &journal, &FixedClock)
                .await
                .err()
                .expect("journal failure must be returned")
                .code(),
            SafeErrorCode::StorageUnavailable
        );
        assert!(identities.list_identities().await.unwrap().is_empty());
    }
}
