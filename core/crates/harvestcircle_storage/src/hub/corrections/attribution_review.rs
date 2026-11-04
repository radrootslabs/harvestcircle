//! Complete current attribution comparisons; candidates never admit or post input.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{
        AttributionAffectedDeparture, AttributionError, OriginalRecipientAttribution,
        OriginalSourceAttribution, RecipientAttributionCommand, RecipientAttributionCorrection,
        SourceAttributionCommand, SourceAttributionCorrection,
    },
    reference::{ReferenceCatalog, SourceAttribution},
};
pub(super) mod registry;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubAttributionReviewOutcome<T> {
    Candidate(Box<T>),
    OperationAlreadyUsed,
    Blocked(AttributionError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubSourceAttributionReview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub proposed_operation: DurableRequestId,
    pub original: OriginalSourceAttribution,
    pub history: Vec<SourceAttributionCorrection>,
    /// Every canonical source identity, retaining distinct IDs and issuers.
    pub known_sources: Vec<SourceAttribution>,
    pub affected: Vec<AttributionAffectedDeparture>,
    pub issued_versions:
        Vec<harvestcircle_domain::hub::corrections::AttributionAffectedIssuedVersion>,
    pub uncertain_intent: bool,
    pub unresolved_actual: bool,
    pub outcome: HubAttributionReviewOutcome<SourceAttributionCorrection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubRecipientAttributionReview {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub proposed_operation: DurableRequestId,
    pub original: OriginalRecipientAttribution,
    pub history: Vec<RecipientAttributionCorrection>,
    pub linked_issued_versions: Vec<HubRecordId>,
    pub uncertain_intent: bool,
    pub unresolved_actual: bool,
    pub outcome: HubAttributionReviewOutcome<RecipientAttributionCorrection>,
}

struct Cut {
    hub: HubId,
    revision: HubRevision,
    catalog: ReferenceCatalog,
    uncertainty: crate::hub::submission::HubUncertainty,
}
async fn cut_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    stock: HubRevision,
    catalog: HubRevision,
) -> Result<Cut, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let state = stock::read::state_on(tx, hub.id()).await?;
    let references = reference::load_catalog_on(tx, hub.id()).await?;
    if state.revision != stock || references.revision() != catalog {
        return Err(stock::conflict());
    }
    // These histories require qualified original/current consumers before a
    // writer can activate them. No absent adapter becomes an empty history.
    let unsupported:i64=sqlx::query_scalar("SELECT count(*) FROM hub_corrections WHERE family IN ('document','count_explain','count_classify')")
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if unsupported != 0 {
        return Err(corrupt_storage());
    }
    let uncertainty = crate::hub::submission::uncertainty_all_on(tx).await?;
    Ok(Cut {
        hub: hub.id(),
        revision: state.revision,
        catalog: references,
        uncertainty,
    })
}

impl Database {
    pub async fn hub_source_attribution_review(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: SourceAttributionCommand,
        now: UnixTimestamp,
    ) -> Result<HubSourceAttributionReview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let cut = cut_on(
                        tx,
                        actor,
                        command.expected_stock_revision,
                        command.expected_catalog_revision,
                    )
                    .await?;
                    let ledger = Box::pin(super::source_ledger::load_on(tx, cut.hub)).await?;
                    let mut context = Box::pin(super::source_context::at_on(
                        tx,
                        cut.hub,
                        &ledger,
                        command.trace,
                        cut.revision,
                        cut.catalog.revision(),
                    ))
                    .await?;
                    context.uncertain = cut.uncertainty.uncertain_intent;
                    let mut review = HubSourceAttributionReview {
                        revision: cut.revision,
                        catalog_revision: cut.catalog.revision(),
                        proposed_operation: operation.clone(),
                        original: context.original.clone(),
                        history: context.history.clone(),
                        known_sources: context.known_sources.clone(),
                        affected: context.affected.clone(),
                        issued_versions: context.issued_versions.clone(),
                        uncertain_intent: cut.uncertainty.uncertain_intent,
                        unresolved_actual: cut.uncertainty.unresolved_actual,
                        outcome: HubAttributionReviewOutcome::OperationAlreadyUsed,
                    };
                    if !crate::hub::submission::operation_exists_on(tx, &operation).await? {
                        let own = crate::hub::actuals::common::operation_id(&operation)?;
                        review.outcome = match command.plan(&context.domain(), own, actor, now) {
                            Ok(plan) => HubAttributionReviewOutcome::Candidate(Box::new(plan)),
                            Err(error) => HubAttributionReviewOutcome::Blocked(error),
                        };
                    }
                    Ok(review)
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    pub async fn hub_recipient_attribution_review(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: RecipientAttributionCommand,
        now: UnixTimestamp,
    ) -> Result<HubRecipientAttributionReview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let cut = cut_on(
                        tx,
                        actor,
                        command.expected_stock_revision,
                        command.expected_catalog_revision,
                    )
                    .await?;
                    let target =
                        crate::hub::actuals::common::operation(command.departure_operation)?;
                    let native = stock::read::operation_on(tx, cut.hub, &target)
                        .await?
                        .ok_or_else(stock::invalid_input)?;
                    match native.kind.as_str() {
                        "departure" => {}
                        "actual_review" => {
                            let receipt = Box::pin(crate::hub::actuals::reviews::original_on(
                                tx, cut.hub, native,
                            ))
                            .await?;
                            if !matches!(
                                receipt.original,
                                crate::HubActualReviewOriginal::Departure(_)
                            ) {
                                return Err(stock::invalid_input());
                            }
                        }
                        _ => return Err(stock::invalid_input()),
                    }
                    let ledger = Box::pin(super::recipient_ledger::load_on(tx, cut.hub)).await?;
                    let mut context = Box::pin(super::recipient_context::at_on(
                        tx,
                        cut.hub,
                        &ledger.records,
                        command.departure_operation,
                        cut.revision,
                        cut.catalog.revision(),
                    ))
                    .await?;
                    context.uncertain = cut.uncertainty.uncertain_intent;
                    let original = context.original.clone();
                    let mut review = HubRecipientAttributionReview {
                        revision: cut.revision,
                        catalog_revision: cut.catalog.revision(),
                        proposed_operation: operation.clone(),
                        linked_issued_versions: context.linked_issued_versions.clone(),
                        original,
                        history: context.history.clone(),
                        uncertain_intent: cut.uncertainty.uncertain_intent,
                        unresolved_actual: cut.uncertainty.unresolved_actual,
                        outcome: HubAttributionReviewOutcome::OperationAlreadyUsed,
                    };
                    if !crate::hub::submission::operation_exists_on(tx, &operation).await? {
                        let own = crate::hub::actuals::common::operation_id(&operation)?;
                        review.outcome = match command.plan(&context.domain(), own, actor, now) {
                            Ok(plan) => HubAttributionReviewOutcome::Candidate(Box::new(plan)),
                            Err(error) => HubAttributionReviewOutcome::Blocked(error),
                        };
                    }
                    Ok(review)
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
