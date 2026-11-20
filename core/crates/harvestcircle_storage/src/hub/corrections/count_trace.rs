//! Complete native count interpretation for read-only trace retrieval. No
//! quantity projection or returned link supplies current execution authority.
use super::*;
use harvestcircle_domain::hub::actuals::{
    ActualKnownCountInventory, ActualKnownInterpretation, ActualKnownInterpretations,
};
use harvestcircle_domain::hub::corrections::CountReturnAssociation;

pub(in crate::hub) struct Counts {
    pub inventory: Option<ActualKnownCountInventory>,
    pub associations: Vec<CountReturnAssociation>,
    pub unresolved_returns: bool,
}

pub(in crate::hub) async fn load_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    trace: HubRecordId,
    stock: HubRevision,
    catalog: HubRevision,
) -> Result<Counts, SafeError> {
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 21 {
        return Ok(Counts {
            inventory: None,
            associations: Vec::new(),
            unresolved_returns: false,
        });
    }
    let known = Box::pin(super::known::load_with_proofs_on(tx, hub, trace, stock)).await?;
    let latest: i64 = sqlx::query_scalar("SELECT COALESCE(max(recorded_at_unix_s),0) FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=?")
        .bind(hub.as_bytes().as_slice()).bind(i64::try_from(stock.value()).map_err(|_| corrupt_storage())?)
        .fetch_one(&mut **tx).await.map_err(|_| storage_unavailable())?;
    let unresolved_returns = known.iter().any(|record| record.count_return_uncertainty);
    let inventory = ActualKnownCountInventory::new(ActualKnownInterpretations {
        hub,
        stock_revision: stock,
        catalog_revision: catalog,
        latest_recorded_at: UnixTimestamp::from_seconds(latest).ok_or_else(corrupt_storage)?,
        records: known
            .into_iter()
            .filter(|record| record.count_explanation.is_some())
            .map(|record| ActualKnownInterpretation {
                stock_revision: stock,
                catalog_revision: catalog,
                latest_recorded_at: UnixTimestamp::from_seconds(latest).expect("checked"),
                original: record.original,
                recipient_original: record.recipient_original,
                recipient_history: record.recipient_history,
                quantities: record.quantities,
                count_explanation: record.count_explanation,
            })
            .collect(),
    })
    .map_err(|_| corrupt_storage())?;
    // Authenticate the whole canonical prefix before selecting either side of
    // a link. Depletion and later classification never erase earlier links.
    let associations = if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION >= 22 {
        Box::pin(super::count_return_history::associations_on(tx, hub, stock))
            .await?
            .into_iter()
            .filter(|row| row.explanation_trace == trace || row.returned_population == trace)
            .collect()
    } else {
        Vec::new()
    };
    Ok(Counts {
        inventory: Some(inventory),
        associations,
        unresolved_returns,
    })
}
