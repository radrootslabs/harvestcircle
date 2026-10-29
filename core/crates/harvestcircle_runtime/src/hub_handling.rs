//! Bounded native handling with current-session and credential authority.
use super::*;
use harvestcircle_domain::hub::departure::DepartureCommand;
use harvestcircle_domain::hub::dispatch::DispatchCommand;
use harvestcircle_domain::hub::movement::HandlingCommand;
use harvestcircle_domain::hub::reference::HubRecordId;
use harvestcircle_storage::{
    HubDepartureReceipt, HubDepartureReview, HubDepartureReviewInput, HubDiscrepancyRow,
    HubDispatchPage, HubDispatchQuery, HubDispatchReceipt, HubDispatchView, HubGroupView,
    HubHandlingPage, HubHandlingQuery, HubHandlingReceipt, HubLineagePage, HubLineageQuery,
    HubTreatmentPage, HubTreatmentQuery, HubTreatmentScope, HubTreatmentSnapshot, HubWorkRow,
};

pub(super) enum HubHandlingCommand {
    Trace(Box<super::hub_trace::HubTraceCommand>),
    DepartureReview(HubDepartureReviewInput),
    DepartureOperation(DurableRequestId),
    ApplyDeparture {
        operation: DurableRequestId,
        command: Box<DepartureCommand>,
    },
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
    pub(super) const fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::Apply { .. } | Self::ApplyDispatch { .. } | Self::ApplyDeparture { .. }
        )
    }

    pub(super) const fn class(&self) -> RuntimeCommandClass {
        if self.is_mutating() || matches!(self, Self::DepartureReview(_)) {
            RuntimeCommandClass::UseCredential
        } else {
            RuntimeCommandClass::Observe
        }
    }
}

pub(super) enum HubHandlingValue {
    Trace(super::hub_trace::HubTraceValue),
    DepartureReview(Box<HubDepartureReview>),
    DepartureReceipt(Option<Box<HubDepartureReceipt>>),
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
                        HubHandlingCommand::Trace(command) => HubHandlingValue::Trace(
                            super::hub_trace::execute(adapter.database(), actor, *command).await?,
                        ),
                        HubHandlingCommand::DepartureReview(request) => {
                            hub_authority::require_owner_credential(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                actor,
                            )
                            .await?;
                            HubHandlingValue::DepartureReview(Box::new(
                                adapter
                                    .database()
                                    .hub_departure_review(actor, request, clock.now())
                                    .await?,
                            ))
                        }
                        HubHandlingCommand::DepartureOperation(operation) => {
                            HubHandlingValue::DepartureReceipt(
                                adapter
                                    .database()
                                    .hub_departure_operation(actor, operation)
                                    .await?
                                    .map(Box::new),
                            )
                        }
                        HubHandlingCommand::ApplyDeparture { operation, command } => {
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::Departure(command),
                            )
                            .await?;
                            let HubMutationResult::Departure(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubHandlingValue::DepartureReceipt(Some(receipt))
                        }

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
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::Dispatch(command),
                            )
                            .await?;
                            let HubMutationResult::Dispatch(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubHandlingValue::DispatchReceipt(Some(receipt))
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
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::Handling(command),
                            )
                            .await?;
                            let HubMutationResult::Handling(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubHandlingValue::Receipt(Some(receipt))
                        }
                    };
                    Ok(RuntimeCommandValue::HubHandling(value))
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
