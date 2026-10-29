//! A stable current comparison cut; never a posting permission or stock mutation.
use super::*;
use crate::hub::submission;
use harvestcircle_domain::hub::{
    counts::{CountContext, CountReview},
    movement::PhysicalStockGroup,
    reference::ReferenceCatalog,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountReview {
    pub observation: CountObservation,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub current: PhysicalStockGroup,
    pub assessment: CountReview,
}

/// Owned current facts keep review and effect planning on the same native path.
pub(super) struct CurrentCountFacts {
    pub observation: CountObservation,
    pub state: stock::read::StockState,
    pub catalog: ReferenceCatalog,
    pub group: PhysicalStockGroup,
    latest_physical_recorded_at: UnixTimestamp,
    previous_reconciliation: Option<HubRecordId>,
    latest_observation: Option<HubRecordId>,
    uncertainty: submission::HubUncertainty,
}

impl CurrentCountFacts {
    pub fn context(&self) -> CountContext<'_> {
        CountContext {
            hub: self.observation.scope.hub,
            stock_revision: self.state.revision,
            catalog: &self.catalog,
            group: &self.group,
            latest_physical_recorded_at: self.latest_physical_recorded_at,
            previous_reconciliation: self.previous_reconciliation,
            latest_observation: self.latest_observation,
            unresolved_actual: self.uncertainty.unresolved_actual,
            uncertain_intent: self.uncertainty.uncertain_intent,
        }
    }
}

impl Database {
    pub async fn hub_count_review(
        &self,
        actor: PublicKey,
        count: HubRecordId,
    ) -> Result<HubCountReview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    review_on(tx, hub.id(), count).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

async fn review_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    count: HubRecordId,
) -> Result<HubCountReview, SafeError> {
    let facts = load_on(tx, hub, count, None).await?;
    // Both sides came from validated native history/current reads. An invalid
    // comparison is damaged state, rather than a new client input error.
    let assessment = facts
        .observation
        .review(&facts.context())
        .map_err(|_| corrupt_storage())?;
    Ok(HubCountReview {
        observation: facts.observation,
        stock_revision: facts.state.revision,
        catalog_revision: facts.catalog.revision(),
        current: facts.group,
        assessment,
    })
}

pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    count: HubRecordId,
    executing: Option<&DurableRequestId>,
) -> Result<CurrentCountFacts, SafeError> {
    let receipt = read::by_id_on(tx, hub, count).await?;
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    let group = handling::read::group_on(tx, hub, receipt.original.scope.group, &catalog).await?;
    let uncertainty =
        submission::uncertainty_on(tx, executing.unwrap_or(&receipt.operation)).await?;
    Ok(CurrentCountFacts {
        latest_physical_recorded_at: read::latest_physical_on(tx, group.id).await?,
        previous_reconciliation: read::previous_reconciliation_on(tx, group.id).await?,
        latest_observation: latest_observation_on(tx, hub, group.id).await?,
        observation: receipt.original,
        state,
        catalog,
        group,
        uncertainty,
    })
}

async fn latest_observation_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    group: HubRecordId,
) -> Result<Option<HubRecordId>, SafeError> {
    let bytes: Option<Vec<u8>> = sqlx::query_scalar(concat!(
        "SELECT substr(c.count_id,1,17) FROM hub_count_observations c JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE c.group_id=? AND o.hub_id=? AND o.kind='count_observe' ORDER BY o.stock_revision DESC LIMIT 1"
    )).bind(group.as_bytes().as_slice()).bind(hub.as_bytes().as_slice())
        .fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    bytes
        .map(|value| {
            HubRecordId::from_bytes(value.try_into().map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())
        })
        .transpose()
}
