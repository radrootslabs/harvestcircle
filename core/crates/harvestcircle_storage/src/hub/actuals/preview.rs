//! Current read-only review data. These plans carry no durable admission or grant.
use super::{common::*, frozen, model::*};
use crate::{
    Database,
    db::map_transaction_error,
    hub::{reference, stock},
};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::{
    PublicKey, SafeError, UnixTimestamp,
    hub::actuals::{ActualDuplicateLink, ActualNewDepartureCommand, HistoricalHoldScopeContext},
};
use radroots_service_sqlite::ServiceSqliteTransaction;

async fn unused_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
) -> Result<(), SafeError> {
    let exists = crate::hub::submission::operation_exists_on(tx, operation).await?;
    if exists {
        return Err(stock::conflict());
    }
    Ok(())
}
impl Database {
    /// Evaluates current native facts without saving input, reserving capacity,
    /// retiring work or admitting a mutation. Submission independently rechecks.
    pub async fn hub_actual_departure_preview(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: ActualNewDepartureCommand,
        now: UnixTimestamp,
    ) -> Result<HubActualDeparturePreview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    unused_on(tx, &operation).await?;
                    let context = frozen::current_on(tx, hub.id(), &command, None).await?;
                    let plan = context.plan(&command, operation_id(&operation)?, actor, now)?;
                    let historical_holds = context
                        .treatment
                        .holds()
                        .iter()
                        .map(|hold| {
                            HistoricalHoldScopeContext {
                                lineage: &context.lineage,
                                group: command.facts.group,
                                occurred: command.facts.occurred,
                                births: &context.births,
                                now,
                            }
                            .verify(hold)
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(error)?;
                    Ok(HubActualDeparturePreview {
                        revision: context.state.revision,
                        catalog_revision: context.catalog.revision(),
                        plan,
                        historical_holds,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
    /// Evaluates an explicit immutable known-event selection; no second physical
    /// event is proposed. A preview never closes the unresolved report.
    pub async fn hub_actual_link_preview(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: ActualDuplicateLink,
        now: UnixTimestamp,
    ) -> Result<HubActualLinkPreview, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    unused_on(tx, &operation).await?;
                    let context = super::links::current_on(tx, hub.id(), &command).await?;
                    let plan = context.plan(&command, operation_id(&operation)?, actor, now)?;
                    Ok(HubActualLinkPreview {
                        revision: context.state.revision,
                        catalog_revision: context.catalog.revision(),
                        plan,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
