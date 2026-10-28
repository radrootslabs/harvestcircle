//! Bounded native handling with current-session and credential authority.
use super::*;
use harvestcircle_domain::hub::dispatch::DispatchCommand;
use harvestcircle_domain::hub::movement::HandlingCommand;
use harvestcircle_domain::hub::reference::HubRecordId;
use harvestcircle_storage::{
    HubDiscrepancyRow, HubDispatchPage, HubDispatchQuery, HubDispatchReceipt, HubDispatchView,
    HubGroupView, HubHandlingPage, HubHandlingQuery, HubHandlingReceipt, HubLineagePage,
    HubLineageQuery, HubTreatmentPage, HubTreatmentQuery, HubTreatmentScope, HubTreatmentSnapshot,
    HubWorkRow,
};

pub(super) enum HubHandlingCommand {
    DispatchPage(HubDispatchQuery),
    DispatchRecord(HubRecordId),
    DispatchOperation(DurableRequestId),
    ApplyDispatch {
        operation: DurableRequestId,
        command: Box<DispatchCommand>,
    },
    Group(HubRecordId),
    Lineage(HubLineageQuery),
    Treatment(HubTreatmentScope),
    TreatmentHistory(HubTreatmentQuery),
    Work(HubHandlingQuery),
    Discrepancies(HubHandlingQuery),
    Apply {
        operation: DurableRequestId,
        command: Box<HandlingCommand>,
    },
    Operation(DurableRequestId),
}

impl HubHandlingCommand {
    pub(super) const fn class(&self) -> RuntimeCommandClass {
        if matches!(self, Self::Apply { .. } | Self::ApplyDispatch { .. }) {
            RuntimeCommandClass::UseCredential
        } else {
            RuntimeCommandClass::Observe
        }
    }
}

pub(super) enum HubHandlingValue {
    DispatchPage(Box<HubDispatchPage>),
    DispatchRecord(Box<HubDispatchView>),
    DispatchReceipt(Option<Box<HubDispatchReceipt>>),
    Group(Box<HubGroupView>),
    Lineage(Box<HubLineagePage>),
    Treatment(Box<HubTreatmentSnapshot>),
    TreatmentHistory(Box<HubTreatmentPage>),
    Work(Box<HubHandlingPage<HubWorkRow>>),
    Discrepancies(Box<HubHandlingPage<HubDiscrepancyRow>>),
    Receipt(Option<Box<HubHandlingReceipt>>),
}

impl RuntimeActorHandle {
    pub async fn hub_treatment(
        &self,
        scope: HubTreatmentScope,
    ) -> Result<HubTreatmentSnapshot, SafeError> {
        match self
            .dispatch_handling(HubHandlingCommand::Treatment(scope))
            .await?
        {
            HubHandlingValue::Treatment(snapshot) => Ok(*snapshot),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_treatment_history(
        &self,
        query: HubTreatmentQuery,
    ) -> Result<HubTreatmentPage, SafeError> {
        query.validate()?;
        match self
            .dispatch_handling(HubHandlingCommand::TreatmentHistory(query))
            .await?
        {
            HubHandlingValue::TreatmentHistory(page) => Ok(*page),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_lineage(&self, query: HubLineageQuery) -> Result<HubLineagePage, SafeError> {
        query.validate()?;
        match self
            .dispatch_handling(HubHandlingCommand::Lineage(query))
            .await?
        {
            HubHandlingValue::Lineage(page) => Ok(*page),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_group(&self, id: HubRecordId) -> Result<HubGroupView, SafeError> {
        match self
            .dispatch_handling(HubHandlingCommand::Group(id))
            .await?
        {
            HubHandlingValue::Group(group) => Ok(*group),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_work(
        &self,
        query: HubHandlingQuery,
    ) -> Result<HubHandlingPage<HubWorkRow>, SafeError> {
        query.validate()?;
        match self
            .dispatch_handling(HubHandlingCommand::Work(query))
            .await?
        {
            HubHandlingValue::Work(page) => Ok(*page),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_discrepancies(
        &self,
        query: HubHandlingQuery,
    ) -> Result<HubHandlingPage<HubDiscrepancyRow>, SafeError> {
        query.validate()?;
        match self
            .dispatch_handling(HubHandlingCommand::Discrepancies(query))
            .await?
        {
            HubHandlingValue::Discrepancies(page) => Ok(*page),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn apply_hub_handling(
        &self,
        operation: DurableRequestId,
        command: HandlingCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubHandlingReceipt, SafeError> {
        command.validate_shape().map_err(|_| {
            SafeError::new(
                SafeErrorCode::HubInvalidInput,
                SafeMessage::new("Review the bounded identified handling scope."),
            )
        })?;
        match self
            .dispatch_durable(
                RuntimeCommand::HubHandling {
                    generation: self.session_generation(),
                    command: HubHandlingCommand::Apply {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubHandling(HubHandlingValue::Receipt(Some(receipt))) => {
                Ok(*receipt)
            }
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_handling_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubHandlingReceipt>, SafeError> {
        match self
            .dispatch_handling(HubHandlingCommand::Operation(operation))
            .await?
        {
            HubHandlingValue::Receipt(receipt) => Ok(receipt.map(|receipt| *receipt)),
            _ => Err(invalid_actor_response()),
        }
    }
    pub(super) async fn dispatch_handling(
        &self,
        command: HubHandlingCommand,
    ) -> Result<HubHandlingValue, SafeError> {
        match self
            .dispatch(
                RuntimeCommand::HubHandling {
                    generation: self.session_generation(),
                    command,
                },
                None,
            )
            .await?
        {
            RuntimeCommandValue::HubHandling(value) => Ok(value),
            _ => Err(invalid_actor_response()),
        }
    }
}

impl RuntimeActor {
    pub(super) async fn execute_hub_handling(
        &self,
        generation: SessionGeneration,
        context: CommandContext,
        command: HubHandlingCommand,
    ) -> Result<RuntimeCommandValue, SafeError> {
        let actor = self
            .hub_actor(generation)
            .map_err(|_| hub_authority::denied())?;
        let mutating = matches!(
            command,
            HubHandlingCommand::Apply { .. } | HubHandlingCommand::ApplyDispatch { .. }
        );
        let result = self
            .run_async(
                context.deadline(),
                move |adapter, secrets, clock| async move {
                    let value = match command {
                        HubHandlingCommand::DispatchPage(query) => HubHandlingValue::DispatchPage(
                            Box::new(adapter.database().hub_dispatch_page(actor, query).await?),
                        ),
                        HubHandlingCommand::DispatchRecord(id) => HubHandlingValue::DispatchRecord(
                            Box::new(adapter.database().hub_dispatch_record(actor, id).await?),
                        ),
                        HubHandlingCommand::DispatchOperation(operation) => {
                            HubHandlingValue::DispatchReceipt(
                                adapter
                                    .database()
                                    .hub_dispatch_operation(actor, operation)
                                    .await?
                                    .map(Box::new),
                            )
                        }
                        HubHandlingCommand::ApplyDispatch { operation, command } => {
                            hub_authority::require_owner_credential(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                actor,
                            )
                            .await?;
                            HubHandlingValue::DispatchReceipt(Some(Box::new(
                                adapter
                                    .database()
                                    .apply_hub_dispatch(actor, operation, *command, clock.now())
                                    .await?,
                            )))
                        }
                        HubHandlingCommand::Treatment(scope) => HubHandlingValue::Treatment(
                            Box::new(adapter.database().hub_treatment(actor, scope).await?),
                        ),
                        HubHandlingCommand::TreatmentHistory(query) => {
                            HubHandlingValue::TreatmentHistory(Box::new(
                                adapter
                                    .database()
                                    .hub_treatment_history(actor, query)
                                    .await?,
                            ))
                        }
                        HubHandlingCommand::Group(id) => HubHandlingValue::Group(Box::new(
                            adapter.database().hub_group(actor, id).await?,
                        )),
                        HubHandlingCommand::Lineage(query) => HubHandlingValue::Lineage(Box::new(
                            adapter.database().hub_lineage(actor, query).await?,
                        )),
                        HubHandlingCommand::Work(query) => HubHandlingValue::Work(Box::new(
                            adapter.database().hub_work_page(actor, query).await?,
                        )),
                        HubHandlingCommand::Discrepancies(query) => {
                            HubHandlingValue::Discrepancies(Box::new(
                                adapter
                                    .database()
                                    .hub_discrepancy_page(actor, query)
                                    .await?,
                            ))
                        }
                        HubHandlingCommand::Operation(operation) => HubHandlingValue::Receipt(
                            adapter
                                .database()
                                .hub_handling_operation(actor, operation)
                                .await?
                                .map(Box::new),
                        ),
                        HubHandlingCommand::Apply { operation, command } => {
                            hub_authority::require_owner_credential(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                actor,
                            )
                            .await?;
                            let receipt = match *command {
                                HandlingCommand::Treatment(command) => {
                                    adapter
                                        .database()
                                        .apply_hub_treatment(actor, operation, command, clock.now())
                                        .await?
                                }
                                HandlingCommand::Move(command) => {
                                    adapter
                                        .database()
                                        .apply_hub_movement(actor, operation, command, clock.now())
                                        .await?
                                }
                                HandlingCommand::Split(command) => {
                                    adapter
                                        .database()
                                        .apply_hub_split(actor, operation, command, clock.now())
                                        .await?
                                }
                                HandlingCommand::Replan(command) => {
                                    adapter
                                        .database()
                                        .apply_hub_replan(actor, operation, command, clock.now())
                                        .await?
                                }
                                HandlingCommand::NewWork(command) => {
                                    adapter
                                        .database()
                                        .apply_hub_new_work(actor, operation, command, clock.now())
                                        .await?
                                }
                                HandlingCommand::Discrepancy(command) => {
                                    adapter
                                        .database()
                                        .apply_hub_discrepancy(
                                            actor,
                                            operation,
                                            command,
                                            clock.now(),
                                        )
                                        .await?
                                }
                            };
                            HubHandlingValue::Receipt(Some(Box::new(receipt)))
                        }
                    };
                    Ok(RuntimeCommandValue::HubHandling(value))
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
                hub_stock::outcome_unknown()
            } else {
                error
            }
        })
    }
}
