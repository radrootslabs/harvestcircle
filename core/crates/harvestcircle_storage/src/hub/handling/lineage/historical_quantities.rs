//! Reconstruct an exact physical cut from native births and net physical debits.
//! This restores no work, authority or saved execution grant.
use super::*;
use std::collections::BTreeMap;

pub(in crate::hub) async fn quantities_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
    cut: HubRevision,
) -> Result<PhysicalLineage, SafeError> {
    // Structural roots are inspected before any denormalized group/event trace
    // filter. Otherwise an omitted sibling could inflate its parent's balance
    // while a second audit over that same omitted topology appeared consistent.
    let operations: Vec<String> = sqlx::query_scalar(
        "SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=? AND kind IN ('move','split','correction_found') ORDER BY stock_revision LIMIT ?",
    ).bind(hub.as_bytes().as_slice()).bind(integer(cut)?)
        .bind(i64::try_from(harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY + 1).map_err(|_| corrupt_storage())?)
        .fetch_all(&mut *tx).await.map_err(|_| storage_unavailable())?;
    if operations.len() > harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY {
        return Err(corrupt_storage());
    }
    let mut reader = crate::hub::actuals::proofs::OriginalReader::new(tx);
    for operation in operations {
        let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
        let original = stock::read::operation_on(&mut reader, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        if original.kind == "move" {
            Box::pin(super::super::history::movement_on(
                &mut reader,
                hub,
                original,
            ))
            .await?;
        } else if original.kind == "split" {
            Box::pin(super::super::portions::original_on(
                &mut reader,
                hub,
                original,
            ))
            .await?;
        } else {
            Box::pin(crate::hub::corrections::found_original_with_proofs_on(
                &mut reader,
                hub,
                original,
            ))
            .await?;
        }
    }
    drop(reader);
    let topology = historical_on(tx, hub, trace, cut).await?;
    let population = read::trace_on(tx, hub, trace).await?;
    let mut births = BTreeMap::new();
    // The original source adapter verifies each immutable birth operation and
    // identification. Topology quantities are deliberately never used here.
    for node in topology.nodes() {
        let operation: String = sqlx::query_scalar(
            "SELECT substr(created_operation_id,1,37) FROM hub_group_identification WHERE group_id=?",
        )
        .bind(node.id.as_bytes().as_slice())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| corrupt_storage())?;
        let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
        let birth = Box::pin(super::super::actuals::source_original_on(
            tx,
            hub,
            &population,
            &operation,
            node.id,
        ))
        .await?;
        if birth.stock_revision != node.created_revision
            || birth.group.parent != node.parent
            || birth.group.trace != trace
            || birth.group.variant != topology.variant()
            || birth.group.packs.value() == 0
            || births.insert(node.id, birth.group.packs).is_some()
        {
            return Err(corrupt_storage());
        }
    }
    let outflows = Box::pin(super::super::actuals::physical_outflows_on(
        tx, hub, &topology,
    ))
    .await?;
    let zero = WholePacks::from_value(0).expect("zero");
    let mut detached = BTreeMap::new();
    for node in topology.nodes() {
        if let Some(parent) = node.parent {
            let total = detached.entry(parent).or_insert(zero);
            *total = total
                .checked_add(births[&node.id])
                .map_err(|_| corrupt_storage())?;
        }
    }
    if outflows.keys().any(|id| topology.node(*id).is_none()) {
        return Err(corrupt_storage());
    }
    let mut nodes = topology.nodes().to_vec();
    for node in &mut nodes {
        node.packs = births[&node.id]
            .checked_sub(outflows.get(&node.id).copied().unwrap_or(zero))
            .and_then(|packs| packs.checked_sub(detached.get(&node.id).copied().unwrap_or(zero)))
            .map_err(|_| corrupt_storage())?;
    }
    let graph = PhysicalLineage::new(hub, trace, topology.variant(), cut, nodes)
        .map_err(|_| corrupt_storage())?;
    // Retain the complete existing native birth proof and reverse conservation
    // audit. Found credits are already assigned to their exact physical debit;
    // their structural child birth remains counted once above.
    Box::pin(historical_births_on(tx, hub, &graph)).await?;
    Ok(graph)
}
