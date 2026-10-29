//! Serialized current-session inventory commands. Domain and SQL own consequences.
use super::*;
use harvestcircle_domain::hub::stock::{OpeningCommand, ReceiptCommand};
use harvestcircle_storage::{HubOpeningReceipt, HubStockPage, HubStockQuery, HubStockReceipt};

pub(super) enum HubStockCommand {
    Read(HubStockQuery),
    Receive {
        operation: DurableRequestId,
        command: Box<ReceiptCommand>,
    },
    ReceiptOperation(DurableRequestId),
    Opening {
        operation: DurableRequestId,
        command: Box<OpeningCommand>,
    },
    OpeningOperation(DurableRequestId),
}
impl HubStockCommand {
    pub(super) const fn is_mutating(&self) -> bool {
        matches!(self, Self::Receive { .. } | Self::Opening { .. })
    }

    pub(super) const fn class(&self) -> RuntimeCommandClass {
        if self.is_mutating() {
            RuntimeCommandClass::UseCredential
        } else {
            RuntimeCommandClass::Observe
        }
    }
}
pub(super) enum HubStockValue {
    Page(Box<HubStockPage>),
    Receipt(Option<Box<HubStockReceipt>>),
    Opening(Option<Box<HubOpeningReceipt>>),
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
    pub async fn record_hub_opening(
        &self,
        operation: DurableRequestId,
        command: OpeningCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubOpeningReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubStock {
                    generation: self.session_generation(),
                    command: HubStockCommand::Opening {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubStock(HubStockValue::Opening(Some(receipt))) => Ok(*receipt),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_opening_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubOpeningReceipt>, SafeError> {
        match self
            .dispatch_stock(HubStockCommand::OpeningOperation(operation))
            .await?
        {
            HubStockValue::Opening(receipt) => Ok(receipt.map(|r| *r)),
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
                        HubStockCommand::OpeningOperation(operation) => HubStockValue::Opening(
                            adapter
                                .database()
                                .hub_opening_operation(actor, operation)
                                .await?
                                .map(Box::new),
                        ),
                        HubStockCommand::Opening { operation, command } => {
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::Opening(command),
                            )
                            .await?;
                            let HubMutationResult::Opened(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubStockValue::Opening(Some(receipt))
                        }
                        HubStockCommand::Receive { operation, command } => {
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::Receive(command),
                            )
                            .await?;
                            let HubMutationResult::Received(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubStockValue::Receipt(Some(receipt))
                        }
                    };
                    Ok(RuntimeCommandValue::HubStock(value))
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
