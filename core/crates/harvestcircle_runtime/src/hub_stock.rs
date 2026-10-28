//! Serialized current-session inventory commands. Domain and SQL own consequences.
use super::*;
use harvestcircle_domain::hub::stock::ReceiptCommand;
use harvestcircle_storage::{HubStockPage, HubStockQuery, HubStockReceipt};

pub(super) enum HubStockCommand {
    Read(HubStockQuery),
    Receive {
        operation: DurableRequestId,
        command: Box<ReceiptCommand>,
    },
    ReceiptOperation(DurableRequestId),
}
impl HubStockCommand {
    pub(super) const fn class(&self) -> RuntimeCommandClass {
        if matches!(self, Self::Receive { .. }) {
            RuntimeCommandClass::UseCredential
        } else {
            RuntimeCommandClass::Observe
        }
    }
}
pub(super) enum HubStockValue {
    Page(Box<HubStockPage>),
    Receipt(Option<Box<HubStockReceipt>>),
}

impl RuntimeActorHandle {
    pub async fn hub_stock(&self, query: HubStockQuery) -> Result<HubStockPage, SafeError> {
        query.validate()?;
        match self.dispatch_stock(HubStockCommand::Read(query)).await? {
            HubStockValue::Page(page) => Ok(*page),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn receive_hub_stock(
        &self,
        operation: DurableRequestId,
        command: ReceiptCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubStockReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubStock {
                    generation: self.session_generation(),
                    command: HubStockCommand::Receive {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubStock(HubStockValue::Receipt(Some(receipt))) => Ok(*receipt),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_receipt_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubStockReceipt>, SafeError> {
        match self
            .dispatch_stock(HubStockCommand::ReceiptOperation(operation))
            .await?
        {
            HubStockValue::Receipt(receipt) => Ok(receipt.map(|r| *r)),
            _ => Err(invalid_actor_response()),
        }
    }
    async fn dispatch_stock(&self, command: HubStockCommand) -> Result<HubStockValue, SafeError> {
        match self
            .dispatch(
                RuntimeCommand::HubStock {
                    generation: self.session_generation(),
                    command,
                },
                None,
            )
            .await?
        {
            RuntimeCommandValue::HubStock(value) => Ok(value),
            _ => Err(invalid_actor_response()),
        }
    }
}

impl RuntimeActor {
    pub(super) async fn execute_hub_stock(
        &self,
        generation: SessionGeneration,
        context: CommandContext,
        command: HubStockCommand,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let actor = self
            .hub_actor(generation)
            .map_err(|_| hub_authority::denied())?;
        let mutating = matches!(command, HubStockCommand::Receive { .. });
        let result = self
            .run_async(
                context.deadline(),
                move |adapter, secrets, clock| async move {
                    let value = match command {
                        HubStockCommand::Read(query) => HubStockValue::Page(Box::new(
                            adapter.database().hub_stock_page(actor, query).await?,
                        )),
                        HubStockCommand::ReceiptOperation(operation) => HubStockValue::Receipt(
                            adapter
                                .database()
                                .hub_receipt_operation(actor, operation)
                                .await?
                                .map(Box::new),
                        ),
                        HubStockCommand::Receive { operation, command } => {
                            hub_authority::require_owner_credential(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                actor,
                            )
                            .await?;
                            HubStockValue::Receipt(Some(Box::new(
                                adapter
                                    .database()
                                    .apply_hub_receipt(actor, operation, *command, clock.now())
                                    .await?,
                            )))
                        }
                    };
                    Ok(RuntimeCommandValue::HubStock(value))
                },
            )
            .await;
        result.map_err(|error| {
            if mutating
                && matches!(
                    error.code(),
                    SafeErrorCode::StorageUnavailable | SafeErrorCode::InvalidApplicationState
                )
            {
                outcome_unknown()
            } else {
                error
            }
        })
    }
}

pub(super) const fn outcome_unknown() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubOutcomeUnknown,
        SafeMessage::new(
            "The inventory operation outcome is unconfirmed. Check its original operation ID before another submission.",
        ),
    )
}
