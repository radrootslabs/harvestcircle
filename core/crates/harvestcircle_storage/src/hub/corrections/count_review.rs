//! Read-only explanation of an already posted shortage. No second physical debit.
use super::*;
use harvestcircle_domain::hub::{
    actuals::{ActualSourceContext, CountedShortageSourceContext},
    corrections::{
        CountExplanationContext, CountExplanationFacts, CountExplanationPlan,
        CountLossQuantityAccount, OriginalCountLossQuantity, QuantityEffectError,
    },
    counts::CountEffect,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountExplanationQuery {
    pub facts: CountExplanationFacts,
    pub source_operation: HubRecordId,
    pub source_group: HubRecordId,
}

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
        self.host().transaction(|tx| Box::pin(async move {
            let hub = reference::authorize_on(tx, actor).await?.id();
            reference::validate_basis_on(tx, hub).await?;
            let state = stock::read::state_on(tx, hub).await?;
            let catalog = reference::load_catalog_on(tx, hub).await?;
            if state.revision != query.facts.expected_stock_revision
                || catalog.revision() != query.facts.expected_catalog_revision
            {
                return Err(stock::conflict());
            }
            // Writers and their original/correction ledgers are not activated
            // yet. An absent adapter must never fabricate an empty history.
            let unsupported: i64 = sqlx::query_scalar(concat!(
                "SELECT (SELECT count(*) FROM hub_corrections WHERE family IN ('count_explain','count_classify'))+",
                "(SELECT count(*) FROM hub_correction_count_explanations)"
            )).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
            if unsupported != 0 { return Err(corrupt_storage()); }
            let target: Option<String> = sqlx::query_scalar(
                "SELECT substr(operation_id,1,37) FROM hub_count_reconciliations WHERE reconciliation_id=?"
            ).bind(query.facts.count_reconciliation.as_bytes().as_slice())
                .fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?;
            let target = DurableRequestId::parse(target.ok_or_else(stock::invalid_input)?)
                .map_err(|_| corrupt_storage())?;
            let original = stock::read::operation_on(tx, hub, &target).await?
                .ok_or_else(corrupt_storage)?;
            let count = Box::pin(crate::hub::counts::posting::original_on(tx, hub, original)).await?.original;
            if count.id != query.facts.count_reconciliation { return Err(corrupt_storage()); }
            let CountEffect::Shortage { packs, .. } = count.effect else { return Err(stock::invalid_input()); };
            // Current materialized positions are independently audited before
            // offering a current comparison. A balanced sibling transfer cannot
            // hide behind population-wide conservation or the old count cut.
            let current = Box::pin(handling::lineage::load_on(
                tx, hub, count.before.trace, &catalog, state.revision,
            )).await?;
                    let reconstructed_current = Box::pin(handling::lineage::historical_quantities_on(
                        tx, hub, count.before.trace, state.revision,
                    )).await?;
                    if current.graph != reconstructed_current {
                        return Err(corrupt_storage());
                    }
            let account = CountLossQuantityAccount::restore(OriginalCountLossQuantity {
                reconciliation: count.id, operation: count.operation,
                stock_revision: count.stock_revision, trace: count.before.trace,
                variant: count.before.variant, packs,
            }, Vec::new()).map_err(|_| corrupt_storage())?;
            let report = crate::hub::actuals::reports::by_id_on(tx, hub, query.facts.report).await?.original;
            let prior: Option<String> = sqlx::query_scalar(
                "SELECT substr(review_operation_id,1,37) FROM hub_actual_report_reviews WHERE report_id=?"
            ).bind(report.id.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
            let prior_report_review = prior.map(|id| {
                DurableRequestId::parse(id).map_err(|_| corrupt_storage()).and_then(|id|
                    crate::hub::actuals::common::operation_id(&id).map_err(|_| corrupt_storage()))
            }).transpose()?;
            let graph = Box::pin(handling::lineage::historical_quantities_on(
                tx, hub, count.before.trace, count.count.scope.stock_revision,
            )).await?;
            let population = handling::read::trace_on(tx, hub, graph.trace()).await?;
            let source = Box::pin(handling::actuals::source_on(
                tx, hub, &graph, &population,
                &crate::hub::actuals::common::operation(query.source_operation)?, query.source_group,
            )).await?;
            let activity = Box::pin(handling::actuals::activity_on(tx, hub, &graph, &source)).await?;
            let treatment = Box::pin(handling::treatment::read::historical_state_on(tx, hub, &graph)).await?;
            let known = Box::pin(super::known::load_on(tx, hub, graph.trace(), state.revision)).await?;
            let latest: i64 = sqlx::query_scalar("SELECT max(recorded_at_unix_s) FROM hub_stock_operations WHERE hub_id=?")
                .bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
            let uncertain = crate::hub::submission::uncertainty_all_on(tx).await?.uncertain_intent;
            let mut review = HubCountExplanationReview {
                revision: state.revision, catalog_revision: catalog.revision(),
                proposed_operation: operation.clone(), outcome: HubCountExplanationReviewOutcome::OperationAlreadyUsed,
            };
            if !crate::hub::submission::operation_exists_on(tx, &operation).await? {
                let context = CountExplanationContext {
                    hub, stock_revision: state.revision,
                    latest_recorded_at: UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?,
                    catalog: &catalog, account: &account, report: &report, prior_report_review,
                    source: CountedShortageSourceContext { source: ActualSourceContext {
                        source: &source, lineage: &graph, balancing_group: &count.before,
                        subsequent_activity: &activity,
                    }, count: &count }, treatment_at_count: &treatment,
                    known_departures: known.iter().map(|event| event.as_domain()).collect(),
                    uncertain_submissions: uncertain,
                };
                review.outcome = match query.facts.plan(&context,
                    crate::hub::actuals::common::operation_id(&operation)?, actor, now) {
                    Ok(plan) => HubCountExplanationReviewOutcome::Candidate(Box::new(plan)),
                    Err(error) => HubCountExplanationReviewOutcome::Blocked(error),
                };
            }
            Ok(review)
        })).await.map_err(map_transaction_error)
    }
}

#[cfg(test)]
pub(in crate::hub) mod tests;
