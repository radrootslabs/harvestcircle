//! Only a newly committed admission reaches a new effect; saved pending data cannot.
use super::*;
use crate::{
    HubCountObservationReceipt, HubDepartureReceipt, HubDispatchReceipt, HubHandlingReceipt,
    HubOpeningReceipt, HubReferenceReceipt, HubStockReceipt,
};
use harvestcircle_domain::{SafeErrorCode, UnixTimestamp, hub::HubOwnership};

pub enum HubMutationResult {
    Initialized(HubOwnership),
    Reference(Box<HubReferenceReceipt>),
    Received(Box<HubStockReceipt>),
    Opened(Box<HubOpeningReceipt>),
    Handling(Box<HubHandlingReceipt>),
    Dispatch(Box<HubDispatchReceipt>),
    Departure(Box<HubDepartureReceipt>),
    CountObserved(Box<HubCountObservationReceipt>),
}

impl std::fmt::Debug for HubMutationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Initialized(_) => "Initialized",
            Self::Reference(_) => "Reference",
            Self::Received(_) => "Received",
            Self::Opened(_) => "Opened",
            Self::Handling(_) => "Handling",
            Self::Dispatch(_) => "Dispatch",
            Self::Departure(_) => "Departure",
            Self::CountObserved(_) => "CountObserved",
        };
        f.debug_tuple("HubMutationResult").field(&kind).finish()
    }
}

impl Database {
    /// Native caller proves current custody before entry. Admission, fresh scope
    /// and original outcome checks are still performed in protected SQL here.
    pub async fn submit_hub_mutation(
        &self,
        scope: HubSubmissionScope,
        operation: DurableRequestId,
        mutation: HubMutation,
        now: UnixTimestamp,
    ) -> Result<HubMutationResult, SafeError> {
        let decision = self
            .host()
            .transaction(|tx| {
                Box::pin(
                    async move { admission::prepare_on(tx, scope, operation, mutation, now).await },
                )
            })
            .await
            .map_err(map_transaction_error)?;
        match decision {
            admission::Decision::Recorded {
                operation,
                mutation,
            } => execute(self, scope.actor, operation, mutation, now).await,
            admission::Decision::Fresh(grant) => execute_fresh(self, grant, now).await,
        }
    }
}

pub(super) async fn execute_fresh(
    db: &Database,
    grant: admission::FreshAdmission,
    now: UnixTimestamp,
) -> Result<HubMutationResult, SafeError> {
    let operation = grant.operation.clone();
    let result = execute(db, grant.scope.actor, grant.operation, grant.mutation, now).await;
    match result {
        Ok(value) => Ok(value),
        Err(error)
            if matches!(
                error.code(),
                SafeErrorCode::PendingOperationRecoveryRequired | SafeErrorCode::HubOutcomeUnknown
            ) =>
        {
            Err(admission::outcome_unknown())
        }
        Err(error) => {
            // The owning SDK mapper returns PendingOperationRecoveryRequired
            // for every unconfirmed rollback/commit. Other errors from these
            // transaction-only handlers establish no committed effect.
            db.host()
                .transaction(|tx| {
                    Box::pin(async move { admission::reject_on(tx, &operation, error, now).await })
                })
                .await
                .map_err(|_| admission::outcome_unknown())?;
            Err(admission::known_rejection(error.code()))
        }
    }
}

async fn execute(
    db: &Database,
    actor: PublicKey,
    operation: DurableRequestId,
    mutation: HubMutation,
    now: UnixTimestamp,
) -> Result<HubMutationResult, SafeError> {
    use harvestcircle_domain::hub::movement::HandlingCommand;
    Ok(match mutation {
        HubMutation::Initialize(candidate) => {
            HubMutationResult::Initialized(db.initialize_hub(*candidate, &operation, now).await?)
        }
        HubMutation::Reference(command) => HubMutationResult::Reference(Box::new(
            db.apply_hub_reference(actor, operation, *command, now)
                .await?,
        )),
        HubMutation::Receive(command) => HubMutationResult::Received(Box::new(
            db.apply_hub_receipt(actor, operation, *command, now)
                .await?,
        )),
        HubMutation::Opening(command) => HubMutationResult::Opened(Box::new(
            db.apply_hub_opening(actor, operation, *command, now)
                .await?,
        )),
        HubMutation::Dispatch(command) => HubMutationResult::Dispatch(Box::new(
            db.apply_hub_dispatch(actor, operation, *command, now)
                .await?,
        )),
        HubMutation::Departure(command) => HubMutationResult::Departure(Box::new(
            db.apply_hub_departure(actor, operation, *command, now)
                .await?,
        )),
        HubMutation::CountObservation(command) => HubMutationResult::CountObserved(Box::new(
            db.observe_hub_count(actor, operation, *command, now)
                .await?,
        )),
        HubMutation::Handling(command) => {
            let receipt = match *command {
                HandlingCommand::Move(command) => {
                    db.apply_hub_movement(actor, operation, command, now)
                        .await?
                }
                HandlingCommand::Split(command) => {
                    db.apply_hub_split(actor, operation, command, now).await?
                }
                HandlingCommand::Treatment(command) => {
                    db.apply_hub_treatment(actor, operation, command, now)
                        .await?
                }
                HandlingCommand::Replan(command) => {
                    db.apply_hub_replan(actor, operation, command, now).await?
                }
                HandlingCommand::NewWork(command) => {
                    db.apply_hub_new_work(actor, operation, command, now)
                        .await?
                }
                HandlingCommand::Discrepancy(command) => {
                    db.apply_hub_discrepancy(actor, operation, command, now)
                        .await?
                }
            };
            HubMutationResult::Handling(Box::new(receipt))
        }
        // prepare_on refuses this unsupported handler before any admission.
        HubMutation::CountPosting(_) => return Err(stock::invalid_input()),
    })
}
