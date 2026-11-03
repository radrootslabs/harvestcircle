//! Canonical read evidence for reported actuals. No mutation or archived grant.
use super::{handling, reference, stock};
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::{
    HubRevision, actuals::ActualPhysicalBirth, portions::PhysicalLineage, reference::HubRecordId,
    stock::StockPopulation,
};
use harvestcircle_domain::{PublicKey, SafeError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HubActualLineageQuery {
    pub trace: HubRecordId,
    pub expected_revision: Option<HubRevision>,
    pub expected_catalog_revision: Option<HubRevision>,
}

/// At most HUB_LINEAGE_GROUPS canonical births, including depleted groups.
/// These facts are not a future action, stock balance or session permission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualLineageEvidence {
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub population: StockPopulation,
    pub lineage: PhysicalLineage,
    pub births: Vec<ActualPhysicalBirth>,
}
impl Database {
    /// Trusted native read adapter; external clients still require current
    /// runtime identity/credential authority before this storage boundary.
    pub async fn hub_actual_lineage(
        &self,
        actor: PublicKey,
        query: HubActualLineageQuery,
    ) -> Result<HubActualLineageEvidence, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    let state = stock::read::state_on(tx, hub.id()).await?;
                    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
                    if query.expected_revision.is_some_and(|v| v != state.revision)
                        || query
                            .expected_catalog_revision
                            .is_some_and(|v| v != catalog.revision())
                    {
                        return Err(stock::conflict());
                    }
                    let loaded = handling::lineage::load_on(
                        tx,
                        hub.id(),
                        query.trace,
                        &catalog,
                        state.revision,
                    )
                    .await?;
                    let births = handling::lineage::actual_births_on(tx, hub.id(), &loaded).await?;
                    if births.len() != loaded.graph.nodes().len() {
                        return Err(corrupt_storage());
                    }
                    Ok(HubActualLineageEvidence {
                        revision: state.revision,
                        catalog_revision: catalog.revision(),
                        population: loaded.trace,
                        lineage: loaded.graph,
                        births,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

#[cfg(test)]
mod tests;
