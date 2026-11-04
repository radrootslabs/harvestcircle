//! Complete current known meanings are read evidence, never an admission grant.
use crate::{
    Database, HubCorrectionKnownQuery,
    db::map_transaction_error,
    hub::{corrections, reference, stock, submission},
};
use harvestcircle_domain::{PublicKey, SafeError, hub::actuals::ActualKnownInterpretations};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualInterpretationEvidence {
    pub interpretations: ActualKnownInterpretations,
    /// Pending native intent is distinct from unresolved raw actual reports.
    pub uncertain_intent: bool,
    pub unresolved_actual: bool,
}

#[cfg(test)]
mod tests;

impl Database {
    /// Current owner reads every immutable original and current/prior/Unknown
    /// association at one native cut. No caller supplies the clock or inventory.
    /// Even a complete result grants no future effect or saved-input execution.
    pub async fn hub_actual_known_interpretations(
        &self,
        actor: PublicKey,
        query: HubCorrectionKnownQuery,
    ) -> Result<HubActualInterpretationEvidence, SafeError> {
        if query.expected_stock_revision.is_some() != query.expected_catalog_revision.is_some() {
            return Err(stock::invalid_input());
        }
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    let interpretations = Box::pin(corrections::current_actual_interpretations_on(
                        tx,
                        hub.id(),
                        query.trace,
                    ))
                    .await?;
                    if query
                        .expected_stock_revision
                        .is_some_and(|v| v != interpretations.stock_revision)
                        || query
                            .expected_catalog_revision
                            .is_some_and(|v| v != interpretations.catalog_revision)
                    {
                        return Err(stock::conflict());
                    }
                    let uncertainty = submission::uncertainty_all_on(tx).await?;
                    Ok(HubActualInterpretationEvidence {
                        interpretations,
                        uncertain_intent: uncertainty.uncertain_intent,
                        unresolved_actual: uncertainty.unresolved_actual,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
