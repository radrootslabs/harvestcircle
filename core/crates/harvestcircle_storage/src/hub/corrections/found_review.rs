//! Current comparison of one named recorded found observation. It creates no
//! admission, operation, group or future execution permission.
use super::*;
use harvestcircle_domain::hub::corrections::{
    CorrectionObservedFacts, FoundStockPlan, PhysicalCorrectionError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubFoundCorrectionReviewQuery {
    pub observation_operation: DurableRequestId,
    pub proposed_operation: DurableRequestId,
    pub expected_stock_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubFoundCorrectionReviewBlocker {
    ObservationNoLongerCurrent,
    OperationAlreadyUsed,
    TargetNotQualified,
    Policy(PhysicalCorrectionError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubFoundCorrectionReviewOutcome {
    Candidate(Box<FoundStockPlan>),
    Blocked(HubFoundCorrectionReviewBlocker),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubFoundCorrectionReview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub observation: HubCorrectionObservationReceipt,
    pub proposed_operation: DurableRequestId,
    pub outcome: HubFoundCorrectionReviewOutcome,
}

impl Database {
    /// Recomputes current evidence for one exact recorded observation. Even a
    /// candidate is read-only data; posting must reacquire all live native facts.
    pub async fn hub_found_correction_review(
        &self,
        actor: PublicKey,
        query: HubFoundCorrectionReviewQuery,
        now: UnixTimestamp,
    ) -> Result<HubFoundCorrectionReview, SafeError> {
        if query.expected_stock_revision.is_some() != query.expected_catalog_revision.is_some() {
            return Err(stock::invalid_input());
        }
        self.host().transaction(|tx|Box::pin(async move {
            let hub = reference::authorize_on(tx, actor).await?;
            reference::validate_basis_on(tx, hub.id()).await?;
            let state = stock::read::state_on(tx, hub.id()).await?;
            let catalog = reference::load_catalog_on(tx, hub.id()).await?;
            if query.expected_stock_revision.is_some_and(|r| r != state.revision)
                || query.expected_catalog_revision.is_some_and(|r| r != catalog.revision()) {
                return Err(stock::conflict());
            }
            let observed = stock::read::operation_on(tx, hub.id(), &query.observation_operation).await?.ok_or_else(stock::invalid_input)?;
            let observation = super::original_on(tx, hub.id(), observed).await?;
            if !matches!(&observation.original.observation.facts,CorrectionObservedFacts::Found {..}) {return Err(stock::invalid_input());}
            let mut review = HubFoundCorrectionReview {
                revision:state.revision,catalog_revision:catalog.revision(),observation,
                proposed_operation:query.proposed_operation.clone(),
                outcome:HubFoundCorrectionReviewOutcome::Blocked(HubFoundCorrectionReviewBlocker::TargetNotQualified),
            };
            let current:i64=sqlx::query_scalar(concat!(
                "SELECT NOT EXISTS(SELECT 1 FROM hub_correction_observations child WHERE child.previous_operation_id=?) ",
                "AND NOT EXISTS(SELECT 1 FROM hub_corrections used WHERE used.observation_operation_id=?)"
            )).bind(query.observation_operation.as_str()).bind(query.observation_operation.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
            if current != 1 {
                review.outcome=HubFoundCorrectionReviewOutcome::Blocked(HubFoundCorrectionReviewBlocker::ObservationNoLongerCurrent);return Ok(review);
            }
            let used:i64=sqlx::query_scalar(concat!(
                "SELECT EXISTS(SELECT 1 FROM hub_submissions WHERE operation_id=?) ",
                "OR EXISTS(SELECT 1 FROM hub_stock_operations WHERE operation_id=?) ",
                "OR EXISTS(SELECT 1 FROM hub_reference_versions WHERE operation_id=?) ",
                "OR EXISTS(SELECT 1 FROM hub_history WHERE operation_id=?)"
            )).bind(query.proposed_operation.as_str()).bind(query.proposed_operation.as_str()).bind(query.proposed_operation.as_str()).bind(query.proposed_operation.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
            if used != 0 {
                review.outcome=HubFoundCorrectionReviewOutcome::Blocked(HubFoundCorrectionReviewBlocker::OperationAlreadyUsed);return Ok(review);
            }
            let context = match super::found_context::load_on(tx,hub.id(),state.revision,catalog,review.observation.clone(),None).await {
                Ok(value)=>value,
                Err(error) if error.code()==harvestcircle_domain::SafeErrorCode::HubInvalidInput=>return Ok(review),
                Err(error)=>return Err(error),
            };
            let operation=crate::hub::actuals::common::operation_id(&query.proposed_operation).map_err(|_|stock::invalid_input())?;
            review.outcome=match context.plan(operation,actor,now) {
                Ok(plan)=>{
                    super::quantity::ensure_capacity_on(tx,hub.id(),plan.original_target.trace).await?;
                    HubFoundCorrectionReviewOutcome::Candidate(Box::new(plan))
                },
                Err(error)=>HubFoundCorrectionReviewOutcome::Blocked(HubFoundCorrectionReviewBlocker::Policy(error)),
            };
            Ok(review)
        })).await.map_err(map_transaction_error)
    }
}
