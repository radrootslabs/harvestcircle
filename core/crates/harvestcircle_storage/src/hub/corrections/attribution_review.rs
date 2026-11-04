//! Complete current attribution comparisons; candidates never admit or post input.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{
        AttributionAffectedDeparture, AttributionCut, AttributionError,
        OriginalRecipientAttribution, OriginalSourceAttribution, RecipientAttributionCommand,
        RecipientAttributionContext, RecipientAttributionCorrection, SourceAttributionCommand,
        SourceAttributionCorrection,
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
    latest: UnixTimestamp,
    uncertainty: crate::hub::submission::HubUncertainty,
}
impl Cut {
    fn domain(&self) -> AttributionCut<'_> {
        AttributionCut {
            hub: self.hub,
            stock_revision: self.revision,
            catalog: &self.catalog,
            latest_recorded_at: self.latest,
            uncertain_submissions: self.uncertainty.uncertain_intent,
        }
    }
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
    let unsupported:i64=sqlx::query_scalar("SELECT count(*) FROM hub_corrections WHERE family IN ('recipient','document','count_explain','count_classify')")
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if unsupported != 0 {
        return Err(corrupt_storage());
    }
    let latest:i64=sqlx::query_scalar(concat!(
        "SELECT max(latest) FROM (SELECT COALESCE(max(recorded_at_unix_s),0) AS latest FROM hub_stock_operations WHERE hub_id=? ",
        "UNION ALL SELECT COALESCE(max(recorded_at_unix_s),0) AS latest FROM hub_reference_versions WHERE hub_id=?)"
    )).bind(hub.id().as_bytes().as_slice()).bind(hub.id().as_bytes().as_slice())
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let uncertainty = crate::hub::submission::uncertainty_all_on(tx).await?;
    Ok(Cut {
        hub: hub.id(),
        revision: state.revision,
        catalog: references,
        latest: UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?,
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
                    let original =
                        Box::pin(super::known::recipient_on(tx, cut.hub, &target)).await?;
                    let mut review = HubRecipientAttributionReview {
                        revision: cut.revision,
                        catalog_revision: cut.catalog.revision(),
                        proposed_operation: operation.clone(),
                        linked_issued_versions: original.issued_versions.clone(),
                        original,
                        history: Vec::new(),
                        uncertain_intent: cut.uncertainty.uncertain_intent,
                        unresolved_actual: cut.uncertainty.unresolved_actual,
                        outcome: HubAttributionReviewOutcome::OperationAlreadyUsed,
                    };
                    if !crate::hub::submission::operation_exists_on(tx, &operation).await? {
                        let own = crate::hub::actuals::common::operation_id(&operation)?;
                        review.outcome = match command.plan(
                            &RecipientAttributionContext {
                                cut: cut.domain(),
                                original: &review.original,
                                history: &review.history,
                                linked_issued_versions: &review.linked_issued_versions,
                            },
                            own,
                            actor,
                            now,
                        ) {
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
