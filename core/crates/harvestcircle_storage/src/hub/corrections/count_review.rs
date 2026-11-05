//! Read-only explanation of an already posted shortage. No second physical debit.
use super::*;
use harvestcircle_domain::hub::corrections::{
    CountExplanationCommand, CountExplanationPlan, QuantityEffectError,
};

pub type HubCountExplanationQuery = CountExplanationCommand;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubCountExplanationReviewOutcome {
    Candidate(Box<CountExplanationPlan>),
    OperationAlreadyUsed,
    Blocked(QuantityEffectError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountExplanationReview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub proposed_operation: DurableRequestId,
    pub outcome: HubCountExplanationReviewOutcome,
}

impl Database {
    /// Evidence at one protected current cut, never admission or permission to
    /// post. The shortage remains physically accounted by its original count.
    pub async fn hub_count_explanation_review(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        query: HubCountExplanationQuery,
        now: UnixTimestamp,
    ) -> Result<HubCountExplanationReview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?.id();
                    reference::validate_basis_on(tx, hub).await?;
                    let mut reader = OriginalReader::new(tx);
                    let context =
                        Box::pin(super::count_context::current_on(&mut reader, hub, &query))
                            .await?;
                    let mut review = HubCountExplanationReview {
                        revision: context.stock_revision,
                        catalog_revision: context.catalog.revision(),
                        proposed_operation: operation.clone(),
                        outcome: HubCountExplanationReviewOutcome::OperationAlreadyUsed,
                    };
                    if !crate::hub::submission::operation_exists_on(&mut reader, &operation).await?
                    {
                        review.outcome = match query.plan(
                            &context.domain(),
                            crate::hub::actuals::common::operation_id(&operation)?,
                            actor,
                            now,
                        ) {
                            Ok(plan) => HubCountExplanationReviewOutcome::Candidate(Box::new(plan)),
                            Err(error) => HubCountExplanationReviewOutcome::Blocked(error),
                        };
                    }
                    Ok(review)
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

#[cfg(test)]
pub(in crate::hub) mod tests;
