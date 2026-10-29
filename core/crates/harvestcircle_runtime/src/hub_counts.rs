//! Session-bound count observations, review and separately admitted reconciliation.
use super::*;
use harvestcircle_domain::hub::{
    counts::{CountObserveCommand, CountPostCommand},
    reference::HubRecordId,
};
use harvestcircle_storage::{HubCountObservationReceipt, HubCountPostReceipt, HubCountReview};

pub(super) enum HubCountCommand {
    Observe {
        operation: DurableRequestId,
        command: Box<CountObserveCommand>,
    },
    Post {
        operation: DurableRequestId,
        command: Box<CountPostCommand>,
    },
    ObservationOperation(DurableRequestId),
    PostingOperation(DurableRequestId),
    Review(HubRecordId),
}
impl HubCountCommand {
    pub(super) const fn is_mutating(&self) -> bool {
        matches!(self, Self::Observe { .. } | Self::Post { .. })
    }
    pub(super) const fn class(&self) -> RuntimeCommandClass {
        if self.is_mutating() {
            RuntimeCommandClass::UseCredential
        } else {
            RuntimeCommandClass::Observe
        }
    }
}
pub(super) enum HubCountValue {
    Observation(Option<Box<HubCountObservationReceipt>>),
    Posting(Option<Box<HubCountPostReceipt>>),
    Review(Box<HubCountReview>),
}
impl RuntimeActorHandle {
    pub async fn observe_hub_count(
        &self,
        operation: DurableRequestId,
        command: CountObserveCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubCountObservationReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubCount {
                    generation: self.session_generation(),
                    command: HubCountCommand::Observe {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubCount(HubCountValue::Observation(Some(receipt))) => {
                Ok(*receipt)
            }
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn post_hub_count(
        &self,
        operation: DurableRequestId,
        command: CountPostCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubCountPostReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubCount {
                    generation: self.session_generation(),
                    command: HubCountCommand::Post {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubCount(HubCountValue::Posting(Some(receipt))) => Ok(*receipt),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_count_observation_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubCountObservationReceipt>, SafeError> {
        match self
            .dispatch_count(HubCountCommand::ObservationOperation(operation))
            .await?
        {
            HubCountValue::Observation(receipt) => Ok(receipt.map(|r| *r)),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_count_post_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubCountPostReceipt>, SafeError> {
        match self
            .dispatch_count(HubCountCommand::PostingOperation(operation))
            .await?
        {
            HubCountValue::Posting(receipt) => Ok(receipt.map(|r| *r)),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_count_review(&self, count: HubRecordId) -> Result<HubCountReview, SafeError> {
        match self.dispatch_count(HubCountCommand::Review(count)).await? {
            HubCountValue::Review(review) => Ok(*review),
            _ => Err(invalid_actor_response()),
        }
    }
    async fn dispatch_count(&self, command: HubCountCommand) -> Result<HubCountValue, SafeError> {
        match self
            .dispatch(
                RuntimeCommand::HubCount {
                    generation: self.session_generation(),
                    command,
                },
                None,
            )
            .await?
        {
            RuntimeCommandValue::HubCount(value) => Ok(value),
            _ => Err(invalid_actor_response()),
        }
    }
}
impl RuntimeActor {
    pub(super) async fn execute_hub_count(
        &self,
        generation: SessionGeneration,
        context: CommandContext,
        command: HubCountCommand,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let scope = self
            .hub_submission_scope(generation)
            .map_err(|_| hub_authority::denied())?;
        let actor = scope.actor;
        let mutating = command.is_mutating();
        let result = self
            .run_async(
                context.deadline(),
                move |adapter, secrets, clock| async move {
                    let value = match command {
                        HubCountCommand::ObservationOperation(operation) => {
                            HubCountValue::Observation(
                                adapter
                                    .database()
                                    .hub_count_observation_operation(actor, operation)
                                    .await?
                                    .map(Box::new),
                            )
                        }
                        HubCountCommand::PostingOperation(operation) => HubCountValue::Posting(
                            adapter
                                .database()
                                .hub_count_post_operation(actor, operation)
                                .await?
                                .map(Box::new),
                        ),
                        HubCountCommand::Review(count) => HubCountValue::Review(Box::new(
                            adapter.database().hub_count_review(actor, count).await?,
                        )),
                        HubCountCommand::Observe { operation, command } => {
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::CountObservation(command),
                            )
                            .await?;
                            let HubMutationResult::CountObserved(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubCountValue::Observation(Some(receipt))
                        }
                        HubCountCommand::Post { operation, command } => {
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::CountPosting(command),
                            )
                            .await?;
                            let HubMutationResult::CountPosted(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubCountValue::Posting(Some(receipt))
                        }
                    };
                    Ok(RuntimeCommandValue::HubCount(value))
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

#[cfg(test)]
#[path = "hub_counts/tests.rs"]
mod tests;
