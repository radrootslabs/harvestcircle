//! Current native comparisons are descriptive data, never submission admission.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{
        CountClassificationCommand, CountClassificationPlan, CountReturnAssociationCommand,
        CountReturnAssociationError, CountReturnAssociationPlan, QuantityEffectError,
    },
    stock::HUB_TRACE_CAPACITY,
};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubCountClassificationReviewOutcome {
    Candidate(Box<CountClassificationPlan>),
    OperationAlreadyUsed,
    Blocked(QuantityEffectError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountClassificationReview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub proposed_operation: DurableRequestId,
    pub uncertain_intent: bool,
    pub unresolved_return_association: bool,
    pub outcome: HubCountClassificationReviewOutcome,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubCountReturnAssociationReviewOutcome {
    Candidate(Box<CountReturnAssociationPlan>),
    OperationAlreadyUsed,
    Blocked(CountReturnAssociationError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountReturnAssociationReview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub proposed_operation: DurableRequestId,
    pub uncertain_intent: bool,
    pub outcome: HubCountReturnAssociationReviewOutcome,
}

impl Database {
    /// Rebuild the complete current native account and observed facts. A caller
    /// must submit separately and obtain fresh custody and admission to write.
    pub async fn hub_count_classification_review(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: CountClassificationCommand,
        now: UnixTimestamp,
    ) -> Result<HubCountClassificationReview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?.id();
                    reference::validate_basis_on(tx, hub).await?;
                    super::count_mode::epoch_on(tx, hub).await?;
                    let mut reader = OriginalReader::new(tx);
                    let context = Box::pin(super::count_classification::current_on(
                        &mut reader,
                        hub,
                        &command,
                        None,
                    ))
                    .await?;
                    let mut review = HubCountClassificationReview {
                        revision: context.facts.stock,
                        catalog_revision: context.facts.catalog.revision(),
                        proposed_operation: operation.clone(),
                        uncertain_intent: context.uncertain_submissions,
                        unresolved_return_association: context.facts.unresolved_returns,
                        outcome: HubCountClassificationReviewOutcome::OperationAlreadyUsed,
                    };
                    if !crate::hub::submission::operation_exists_on(&mut reader, &operation).await?
                    {
                        let own = crate::hub::actuals::common::operation_id(&operation)?;
                        review.outcome = match command.plan(&context.domain(), own, actor, now) {
                            Ok(plan) => {
                                handling::treatment::capacity::ensure_with_proofs_on(
                            &mut reader, actor, &operation,
                            handling::treatment::capacity::Reservation::CountClassification {
                                report: plan.report.is_some(),
                            },
                        ).await?;
                                super::row_budget::ensure_hub_on(&mut reader).await?;
                                super::row_budget::ensure_on(
                                    &mut reader,
                                    hub,
                                    plan.before.original().trace,
                                )
                                .await?;
                                HubCountClassificationReviewOutcome::Candidate(Box::new(plan))
                            }
                            Err(error) => HubCountClassificationReviewOutcome::Blocked(error),
                        };
                    }
                    Ok(review)
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    /// Compare one complete existing Return against one verified CountE. No
    /// return, association, operation, reservation or acknowledgement is written.
    pub async fn hub_count_return_association_review(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: CountReturnAssociationCommand,
        now: UnixTimestamp,
    ) -> Result<HubCountReturnAssociationReview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?.id();
                    reference::validate_basis_on(tx, hub).await?;
                    let mut reader = OriginalReader::new(tx);
                    let context = Box::pin(super::count_return_context::current_on(
                        &mut reader,
                        hub,
                        &command,
                        None,
                    ))
                    .await?;
                    let mut review = HubCountReturnAssociationReview {
                        revision: context.facts.stock,
                        catalog_revision: context.facts.catalog.revision(),
                        proposed_operation: operation.clone(),
                        uncertain_intent: context.uncertain_submissions,
                        outcome: HubCountReturnAssociationReviewOutcome::OperationAlreadyUsed,
                    };
                    if !crate::hub::submission::operation_exists_on(&mut reader, &operation).await?
                    {
                        let own = crate::hub::actuals::common::operation_id(&operation)?;
                        review.outcome = match command.plan(&context.domain()?, own, actor, now) {
                            Ok(plan) => {
                                super::row_budget::ensure_hub_on(&mut reader).await?;
                                handling::treatment::capacity::ensure_with_proofs_on(
                                    &mut reader,
                                    actor,
                                    &operation,
                                    handling::treatment::capacity::Reservation::OrdinaryMetadata,
                                )
                                .await?;
                                let rows: i64 = sqlx::query_scalar(
                                    "SELECT count(*) FROM hub_count_return_associations",
                                )
                                .fetch_one(&mut *reader)
                                .await
                                .map_err(|_| storage_unavailable())?;
                                if rows < 0 {
                                    return Err(corrupt_storage());
                                }
                                if rows >= HUB_TRACE_CAPACITY as i64 {
                                    return Err(stock::capacity());
                                }
                                super::row_budget::ensure_on(
                                    &mut reader,
                                    hub,
                                    plan.association.explanation_trace,
                                )
                                .await?;
                                HubCountReturnAssociationReviewOutcome::Candidate(Box::new(plan))
                            }
                            Err(error) => HubCountReturnAssociationReviewOutcome::Blocked(error),
                        };
                    }
                    Ok(review)
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
