//! Preparation shares the existing current-session handling actor and journal.
use super::*;
use harvestcircle_domain::hub::dispatch::DispatchCommand;
use harvestcircle_domain::hub::reference::HubRecordId;
use harvestcircle_storage::{
    HubDispatchPage, HubDispatchQuery, HubDispatchReceipt, HubDispatchView,
};

impl RuntimeActorHandle {
    pub async fn hub_dispatch_page(
        &self,
        query: HubDispatchQuery,
    ) -> Result<HubDispatchPage, SafeError> {
        query.validate()?;
        match self
            .dispatch_handling(HubHandlingCommand::DispatchPage(query))
            .await?
        {
            HubHandlingValue::DispatchPage(page) => Ok(*page),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_dispatch_record(&self, id: HubRecordId) -> Result<HubDispatchView, SafeError> {
        match self
            .dispatch_handling(HubHandlingCommand::DispatchRecord(id))
            .await?
        {
            HubHandlingValue::DispatchRecord(record) => Ok(*record),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_dispatch_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubDispatchReceipt>, SafeError> {
        match self
            .dispatch_handling(HubHandlingCommand::DispatchOperation(operation))
            .await?
        {
            HubHandlingValue::DispatchReceipt(receipt) => Ok(receipt.map(|receipt| *receipt)),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn apply_hub_dispatch(
        &self,
        operation: DurableRequestId,
        command: DispatchCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubDispatchReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubHandling {
                    generation: self.session_generation(),
                    command: HubHandlingCommand::ApplyDispatch {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubHandling(HubHandlingValue::DispatchReceipt(Some(receipt))) => {
                Ok(*receipt)
            }
            _ => Err(invalid_actor_response()),
        }
    }
}

impl RuntimeActorHandle {
    pub async fn hub_departure_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<harvestcircle_storage::HubDepartureReceipt>, SafeError> {
        match self
            .dispatch_handling(HubHandlingCommand::DepartureOperation(operation))
            .await?
        {
            HubHandlingValue::DepartureReceipt(receipt) => Ok(receipt.map(|receipt| *receipt)),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn apply_hub_departure(
        &self,
        operation: DurableRequestId,
        command: harvestcircle_domain::hub::departure::DepartureCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<harvestcircle_storage::HubDepartureReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubHandling {
                    generation: self.session_generation(),
                    command: HubHandlingCommand::ApplyDeparture {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubHandling(HubHandlingValue::DepartureReceipt(Some(receipt))) => {
                Ok(*receipt)
            }
            _ => Err(invalid_actor_response()),
        }
    }
}

impl RuntimeActorHandle {
    pub async fn hub_departure_review(
        &self,
        request: harvestcircle_storage::HubDepartureReviewInput,
    ) -> Result<harvestcircle_storage::HubDepartureReview, SafeError> {
        match self
            .dispatch_handling(HubHandlingCommand::DepartureReview(request))
            .await?
        {
            HubHandlingValue::DepartureReview(review) => Ok(*review),
            _ => Err(invalid_actor_response()),
        }
    }
}
