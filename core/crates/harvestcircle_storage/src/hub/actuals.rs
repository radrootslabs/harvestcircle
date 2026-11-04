//! Canonical read evidence for reported actuals. No mutation or archived grant.
pub(in crate::hub) mod common;
mod interpretations;
pub(in crate::hub) mod links;
mod model;
pub use interpretations::HubActualInterpretationEvidence;
mod preview;
pub use model::*;
pub(in crate::hub) mod frozen;
pub(in crate::hub) mod projection;
pub(in crate::hub) mod reports;
pub(in crate::hub) mod reviews;
pub(crate) mod schema;
pub(in crate::hub) mod validation;
pub(in crate::hub) mod write;
use super::{handling, reference, stock};
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::{
    HubRevision,
    actuals::{
        ActualKnownDeparture, ActualPhysicalActivity, ActualPhysicalBirth, HistoricalActualSource,
    },
    portions::PhysicalLineage,
    reference::HubRecordId,
    stock::StockPopulation,
};
use harvestcircle_domain::{PublicKey, SafeError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualSourceQuery {
    pub context: HubActualLineageQuery,
    pub operation: DurableRequestId,
    pub group: HubRecordId,
}

/// Complete canonical evidence for one explicit historical physical witness.
/// No caller supplies an activity list or duplicate-clear flag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubActualSourceEvidence {
    pub lineage: HubActualLineageEvidence,
    pub source: HistoricalActualSource,
    pub subsequent_activity: Vec<ActualPhysicalActivity>,
    pub known_departures: Vec<ActualKnownDeparture>,
}

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
    pub async fn hub_actual_source(
        &self,
        actor: PublicKey,
        query: HubActualSourceQuery,
    ) -> Result<HubActualSourceEvidence, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    let state = stock::read::state_on(tx, hub.id()).await?;
                    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
                    if query
                        .context
                        .expected_revision
                        .is_some_and(|v| v != state.revision)
                        || query
                            .context
                            .expected_catalog_revision
                            .is_some_and(|v| v != catalog.revision())
                    {
                        return Err(stock::conflict());
                    }
                    let loaded = handling::lineage::load_on(
                        tx,
                        hub.id(),
                        query.context.trace,
                        &catalog,
                        state.revision,
                    )
                    .await?;
                    let births = handling::lineage::actual_births_on(tx, hub.id(), &loaded).await?;
                    let source = handling::actuals::source_on(
                        tx,
                        hub.id(),
                        &loaded.graph,
                        &loaded.trace,
                        &query.operation,
                        query.group,
                    )
                    .await?;
                    let subsequent_activity =
                        handling::actuals::activity_on(tx, hub.id(), &loaded.graph, &source)
                            .await?;
                    let known_departures = handling::actuals::known_departures_on(
                        tx,
                        hub.id(),
                        query.context.trace,
                        state.revision,
                    )
                    .await?;
                    Ok(HubActualSourceEvidence {
                        lineage: HubActualLineageEvidence {
                            revision: state.revision,
                            catalog_revision: catalog.revision(),
                            population: loaded.trace,
                            lineage: loaded.graph,
                            births,
                        },
                        source,
                        subsequent_activity,
                        known_departures,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }

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
mod retention_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod upgrade_tests;
