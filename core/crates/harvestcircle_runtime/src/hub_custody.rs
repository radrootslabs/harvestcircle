//! Returned custody and actual removal use native durable admission and session fences.
use super::*;
use harvestcircle_domain::hub::{disposition::DispositionCommand, returns::ReturnCommand};
use harvestcircle_storage::{HubDispositionReceipt, HubReturnReceipt};

impl RuntimeActorHandle {
    pub async fn apply_hub_return(
        &self,
        operation: DurableRequestId,
        command: ReturnCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubReturnReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubHandling {
                    generation: self.session_generation(),
                    command: HubHandlingCommand::ApplyReturn {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubHandling(HubHandlingValue::ReturnReceipt(Some(value))) => {
                Ok(*value)
            }
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_return_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubReturnReceipt>, SafeError> {
        match self
            .dispatch_handling(HubHandlingCommand::ReturnOperation(operation))
            .await?
        {
            HubHandlingValue::ReturnReceipt(value) => Ok(value.map(|v| *v)),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn apply_hub_disposition(
        &self,
        operation: DurableRequestId,
        command: DispositionCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubDispositionReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubHandling {
                    generation: self.session_generation(),
                    command: HubHandlingCommand::ApplyDisposition {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubHandling(HubHandlingValue::DispositionReceipt(Some(value))) => {
                Ok(*value)
            }
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_disposition_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubDispositionReceipt>, SafeError> {
        match self
            .dispatch_handling(HubHandlingCommand::DispositionOperation(operation))
            .await?
        {
            HubHandlingValue::DispositionReceipt(value) => Ok(value.map(|v| *v)),
            _ => Err(invalid_actor_response()),
        }
    }
}
