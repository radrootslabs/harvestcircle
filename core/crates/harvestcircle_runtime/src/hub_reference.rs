//! Session-bound coordination for the local reference-data adapter.

use super::*;
use harvestcircle_domain::hub::reference::{HubRecordId, ReferenceCatalog, ReferenceCommand};
use harvestcircle_storage::HubReferenceReceipt;

pub(super) enum HubReferenceCommand {
    Read,
    Apply {
        operation: DurableRequestId,
        command: ReferenceCommand,
    },
    Operation(DurableRequestId),
    History {
        id: HubRecordId,
        after: u64,
        limit: u32,
    },
}

impl HubReferenceCommand {
    pub(super) const fn is_mutating(&self) -> bool {
        matches!(self, Self::Apply { .. })
    }

    pub(super) const fn class(&self) -> RuntimeCommandClass {
        if self.is_mutating() {
            RuntimeCommandClass::UseCredential
        } else {
            RuntimeCommandClass::Observe
        }
    }
}

pub(super) enum HubReferenceValue {
    Catalog(ReferenceCatalog),
    Receipt(Option<HubReferenceReceipt>),
    History(Vec<HubReferenceReceipt>),
}

impl RuntimeActorHandle {
    pub async fn hub_references(&self) -> Result<ReferenceCatalog, SafeError> {
        match self.dispatch_reference(HubReferenceCommand::Read).await? {
            HubReferenceValue::Catalog(catalog) => Ok(catalog),
            _ => Err(invalid_actor_response()),
        }
    }

    pub async fn apply_hub_reference(
        &self,
        operation: DurableRequestId,
        command: ReferenceCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubReferenceReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubReference {
                    generation: self.session_generation(),
                    command: HubReferenceCommand::Apply { operation, command },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubReference(HubReferenceValue::Receipt(Some(receipt))) => {
                Ok(receipt)
            }
            _ => Err(invalid_actor_response()),
        }
    }

    pub async fn hub_reference_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubReferenceReceipt>, SafeError> {
        match self
            .dispatch_reference(HubReferenceCommand::Operation(operation))
            .await?
        {
            HubReferenceValue::Receipt(receipt) => Ok(receipt),
            _ => Err(invalid_actor_response()),
        }
    }

    pub async fn hub_reference_history(
        &self,
        id: HubRecordId,
        after: u64,
        limit: u32,
    ) -> Result<Vec<HubReferenceReceipt>, SafeError> {
        match self
            .dispatch_reference(HubReferenceCommand::History { id, after, limit })
            .await?
        {
            HubReferenceValue::History(history) => Ok(history),
            _ => Err(invalid_actor_response()),
        }
    }

    async fn dispatch_reference(
        &self,
        command: HubReferenceCommand,
    ) -> Result<HubReferenceValue, SafeError> {
        match self
            .dispatch(
                RuntimeCommand::HubReference {
                    generation: self.session_generation(),
                    command,
                },
                None,
            )
            .await?
        {
            RuntimeCommandValue::HubReference(value) => Ok(value),
            _ => Err(invalid_actor_response()),
        }
    }
}

impl RuntimeActor {
    pub(super) async fn execute_hub_reference(
        &self,
        generation: SessionGeneration,
        context: CommandContext,
        command: HubReferenceCommand,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let scope = self
            .hub_submission_scope(generation)
            .map_err(|_| permission_denied())?;
        let actor = scope.actor;
        let mutating = command.is_mutating();
        let result = self
            .run_async(
                context.deadline(),
                move |adapter, secrets, clock| async move {
                    let value = match command {
                        HubReferenceCommand::Read => HubReferenceValue::Catalog(
                            adapter.database().hub_references(actor).await?,
                        ),
                        HubReferenceCommand::Operation(operation) => HubReferenceValue::Receipt(
                            adapter
                                .database()
                                .hub_reference_operation(actor, operation)
                                .await?,
                        ),
                        HubReferenceCommand::History { id, after, limit } => {
                            HubReferenceValue::History(
                                adapter
                                    .database()
                                    .hub_reference_history(actor, id, after, limit)
                                    .await?,
                            )
                        }
                        HubReferenceCommand::Apply { operation, command } => {
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::Reference(Box::new(command)),
                            )
                            .await?;
                            let HubMutationResult::Reference(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubReferenceValue::Receipt(Some(*receipt))
                        }
                    };
                    Ok(RuntimeCommandValue::HubReference(value))
                },
            )
            .await;
        result.map_err(|error| {
            if mutating {
                hub_authority::mutation_error(error)
            } else {
                error
            }
        })
    }
}

const fn permission_denied() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubPermissionDenied,
        SafeMessage::new("Activate the current hub owner's local identity to access hub records."),
    )
}
