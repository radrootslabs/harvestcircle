//! Complete canonical prefixes. Body ownership is checked before filtering an
//! account, and later bodies are excluded by native operation cuts first.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{CountReturnAssociation, NormalReturnProvenance},
    returns::ReturnPlan,
    stock::HUB_TRACE_CAPACITY,
};

pub(super) async fn associations_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    cut: HubRevision,
) -> Result<Vec<CountReturnAssociation>, SafeError> {
    super::count_return_ledger::epoch_on(tx, hub).await?;
    let through = i64::try_from(cut.value()).map_err(|_| corrupt_storage())?;
    let invalid: i64 = sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_count_return_associations a LEFT JOIN hub_stock_operations o ON o.operation_id=a.operation_id ",
        "WHERE (o.operation_id IS NULL OR o.hub_id<>?1 OR o.stock_revision<=?2) ",
        "AND (o.operation_id IS NULL OR o.hub_id<>?1 OR o.kind<>'count_return_associate')"
    )).bind(hub.as_bytes().as_slice()).bind(through).fetch_one(&mut **tx).await.map_err(|_| storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    let ids: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE kind='count_return_associate' ",
        "AND hub_id=? AND stock_revision<=? ORDER BY stock_revision LIMIT 4097"
    )).bind(hub.as_bytes().as_slice()).bind(through).fetch_all(&mut **tx).await.map_err(|_| storage_unavailable())?;
    if ids.len() > HUB_TRACE_CAPACITY {
        return Err(stock::capacity());
    }
    let mut result = Vec::with_capacity(ids.len());
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind != "count_return_associate" || op.revision.value() > cut.value() {
            return Err(corrupt_storage());
        }
        // A fixed-shape projection is usable only after its full original was
        // authenticated in this same strictly decreasing transaction read phase.
        result.push(Box::pin(tx.count_return_projection(hub, op)).await?);
    }
    Ok(result)
}

pub(super) struct Returned {
    pub operation: HubRecordId,
    pub original: ReturnPlan,
    pub digest: [u8; 32],
}

pub(super) struct Returns {
    pub records: Vec<Returned>,
    pub normal: Vec<NormalReturnProvenance>,
}

pub(super) async fn return_operations_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    cut: HubRevision,
) -> Result<Vec<OperationRecord>, SafeError> {
    super::return_mode::audit_prefix_on(tx, hub, cut).await?;
    let through = i64::try_from(cut.value()).map_err(|_| corrupt_storage())?;
    let invalid: i64 = sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_returns r LEFT JOIN hub_stock_operations o ON o.operation_id=r.operation_id ",
        "WHERE (o.operation_id IS NULL OR o.hub_id<>?1 OR o.stock_revision<=?2) ",
        "AND (o.operation_id IS NULL OR o.hub_id<>?1 OR o.kind<>'return')"
    )).bind(hub.as_bytes().as_slice()).bind(through).fetch_one(&mut **tx).await.map_err(|_| storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    let ids: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE kind='return' ",
        "AND hub_id=? AND stock_revision<=? ORDER BY stock_revision LIMIT 4097"
    ))
    .bind(hub.as_bytes().as_slice())
    .bind(through)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if ids.len() > HUB_TRACE_CAPACITY {
        return Err(stock::capacity());
    }
    let mut result = Vec::with_capacity(ids.len());
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind != "return" || op.revision.value() > cut.value() {
            return Err(corrupt_storage());
        }
        result.push(op);
    }
    Ok(result)
}

pub(super) async fn returns_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    cut: HubRevision,
    selected: HubRecordId,
) -> Result<Returns, SafeError> {
    let mut result = Returns {
        records: Vec::with_capacity(1),
        normal: Vec::new(),
    };
    for op in return_operations_on(tx, hub, cut).await? {
        let own =
            crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
        if own == selected {
            let header = crate::hub::custody::returns::read::own_on(tx, hub, &op).await?;
            let receipt = Box::pin(tx.return_original(hub, op)).await?;
            result.records.push(Returned {
                operation: own,
                original: receipt.original.clone(),
                digest: header.result_hash,
            });
            let op = stock::read::operation_on(tx, hub, &receipt.operation)
                .await?
                .ok_or_else(corrupt_storage)?;
            if let Some(value) = Box::pin(tx.return_provenance(hub, op)).await? {
                result.normal.push(value);
            }
        } else if let Some(value) = Box::pin(tx.return_provenance(hub, op)).await? {
            result.normal.push(value);
        }
    }
    Ok(result)
}
