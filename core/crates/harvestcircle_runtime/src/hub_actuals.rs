//! Session-bound preserved actual reports and explicit historical reconciliation.
use super::*;
use harvestcircle_domain::hub::actuals::{
    ActualDuplicateLink, ActualNewDepartureCommand, ActualReportCommand,
};
use harvestcircle_storage::{
    HubActualReportPage, HubActualReportQuery, HubActualReportReceipt, HubActualReviewReceipt,
    HubActualSourceEvidence, HubActualSourceQuery,
};

pub(super) enum HubActualCommand {
    Report {
        operation: DurableRequestId,
        command: Box<ActualReportCommand>,
    },
    Reconcile {
        operation: DurableRequestId,
        command: Box<ActualNewDepartureCommand>,
    },
    Link {
        operation: DurableRequestId,
        command: Box<ActualDuplicateLink>,
    },
    ReportOperation(DurableRequestId),
    ReviewOperation(DurableRequestId),
    Reports(HubActualReportQuery),
    Source(HubActualSourceQuery),
}
impl HubActualCommand {
    pub(super) const fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::Report { .. } | Self::Reconcile { .. } | Self::Link { .. }
        )
    }
    pub(super) const fn class(&self) -> RuntimeCommandClass {
        if self.is_mutating() {
            RuntimeCommandClass::UseCredential
        } else {
            RuntimeCommandClass::Observe
        }
    }
}
pub(super) enum HubActualValue {
    Report(Option<Box<HubActualReportReceipt>>),
    Review(Option<Box<HubActualReviewReceipt>>),
    Reports(Box<HubActualReportPage>),
    Source(Box<HubActualSourceEvidence>),
}
impl RuntimeActorHandle {
    pub async fn report_hub_actual(
        &self,
        operation: DurableRequestId,
        command: ActualReportCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubActualReportReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubActual {
                    generation: self.session_generation(),
                    command: HubActualCommand::Report {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubActual(HubActualValue::Report(Some(receipt))) => Ok(*receipt),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn reconcile_hub_actual(
        &self,
        operation: DurableRequestId,
        command: ActualNewDepartureCommand,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubActualReviewReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubActual {
                    generation: self.session_generation(),
                    command: HubActualCommand::Reconcile {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubActual(HubActualValue::Review(Some(receipt))) => Ok(*receipt),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn link_hub_actual(
        &self,
        operation: DurableRequestId,
        command: ActualDuplicateLink,
        expected: SnapshotRevision,
        timeout: Duration,
    ) -> Result<HubActualReviewReceipt, SafeError> {
        match self
            .dispatch_durable(
                RuntimeCommand::HubActual {
                    generation: self.session_generation(),
                    command: HubActualCommand::Link {
                        operation,
                        command: Box::new(command),
                    },
                },
                expected,
                timeout,
            )
            .await?
        {
            RuntimeCommandValue::HubActual(HubActualValue::Review(Some(receipt))) => Ok(*receipt),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_actual_report_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubActualReportReceipt>, SafeError> {
        match self
            .dispatch_actual(HubActualCommand::ReportOperation(operation))
            .await?
        {
            HubActualValue::Report(value) => Ok(value.map(|v| *v)),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_actual_review_operation(
        &self,
        operation: DurableRequestId,
    ) -> Result<Option<HubActualReviewReceipt>, SafeError> {
        match self
            .dispatch_actual(HubActualCommand::ReviewOperation(operation))
            .await?
        {
            HubActualValue::Review(value) => Ok(value.map(|v| *v)),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_actual_reports(
        &self,
        query: HubActualReportQuery,
    ) -> Result<HubActualReportPage, SafeError> {
        match self
            .dispatch_actual(HubActualCommand::Reports(query))
            .await?
        {
            HubActualValue::Reports(value) => Ok(*value),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_actual_source(
        &self,
        query: HubActualSourceQuery,
    ) -> Result<HubActualSourceEvidence, SafeError> {
        match self
            .dispatch_actual(HubActualCommand::Source(query))
            .await?
        {
            HubActualValue::Source(value) => Ok(*value),
            _ => Err(invalid_actor_response()),
        }
    }
    async fn dispatch_actual(
        &self,
        command: HubActualCommand,
    ) -> Result<HubActualValue, SafeError> {
        match self
            .dispatch(
                RuntimeCommand::HubActual {
                    generation: self.session_generation(),
                    command,
                },
                None,
            )
            .await?
        {
            RuntimeCommandValue::HubActual(value) => Ok(value),
            _ => Err(invalid_actor_response()),
        }
    }
}
impl RuntimeActor {
    pub(super) async fn execute_hub_actual(
        &self,
        generation: SessionGeneration,
        context: CommandContext,
        command: HubActualCommand,
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
                        HubActualCommand::ReportOperation(operation) => HubActualValue::Report(
                            adapter
                                .database()
                                .hub_actual_report_operation(actor, operation)
                                .await?
                                .map(Box::new),
                        ),
                        HubActualCommand::ReviewOperation(operation) => HubActualValue::Review(
                            adapter
                                .database()
                                .hub_actual_review_operation(actor, operation)
                                .await?
                                .map(Box::new),
                        ),
                        HubActualCommand::Reports(query) => HubActualValue::Reports(Box::new(
                            adapter.database().hub_actual_reports(actor, query).await?,
                        )),
                        HubActualCommand::Source(query) => HubActualValue::Source(Box::new(
                            adapter.database().hub_actual_source(actor, query).await?,
                        )),
                        HubActualCommand::Report { operation, command } => {
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::ActualReport(command),
                            )
                            .await?;
                            let HubMutationResult::ActualReported(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubActualValue::Report(Some(receipt))
                        }
                        HubActualCommand::Reconcile { operation, command } => {
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::ActualReview(command),
                            )
                            .await?;
                            let HubMutationResult::ActualReviewed(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubActualValue::Review(Some(receipt))
                        }
                        HubActualCommand::Link { operation, command } => {
                            let result = hub_authority::submit(
                                adapter.as_ref(),
                                secrets.as_ref(),
                                clock.as_ref(),
                                scope,
                                operation,
                                HubMutation::ActualLink(command),
                            )
                            .await?;
                            let HubMutationResult::ActualReviewed(receipt) = result else {
                                return Err(invalid_actor_response());
                            };
                            HubActualValue::Review(Some(receipt))
                        }
                    };
                    Ok(RuntimeCommandValue::HubActual(value))
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
