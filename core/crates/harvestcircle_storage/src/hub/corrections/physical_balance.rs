//! Cut-specific net physical outflow. Gross originals/events remain unchanged;
//! found credits are deducted exactly once from their own original physical debit.
//! No current lineage or full correction replanner is called from this layer.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{
        DepartureQuantityAmendment, DepartureQuantityDelta, OriginalPhysicalDebit, PhysicalDebitKey,
    },
    reference::WholePacks,
};

pub(in crate::hub) async fn original_physical_net_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &DurableRequestId,
    group: HubRecordId,
    gross: WholePacks,
    cut: HubRevision,
) -> Result<WholePacks, SafeError> {
    // Full immutable proofs compose through several ordinary readers. Keep
    // their future storage on the heap while retaining every proof/check.
    let line = Box::pin(super::quantity::original_on(tx, hub, operation, group, cut)).await?;
    if line.original.packs != gross {
        return Err(corrupt_storage());
    }
    let history = Box::pin(super::quantity::history_from_original_on(
        tx, hub, operation, group, cut, &line,
    ))
    .await?;
    let additional = additional_debits_on(tx, hub, &line, &history.amendments, cut).await?;
    let key = PhysicalDebitKey {
        operation: line.original.operation,
        group,
    };
    let mut recovered = WholePacks::from_value(0).expect("zero");
    let mut keys = std::collections::BTreeSet::from([key]);
    for debit in additional {
        if !keys.insert(debit.key) {
            return Err(corrupt_storage());
        }
    }
    for credit in Box::pin(super::return_allowances::credits_on(
        tx,
        hub,
        &line,
        &history.amendments,
    ))
    .await?
    {
        if !keys.contains(&credit.debit) || credit.stock_revision.value() > cut.value() {
            return Err(corrupt_storage());
        }
        if credit.debit == key {
            recovered = recovered
                .checked_add(credit.packs)
                .map_err(|_| corrupt_storage())?;
        }
    }
    // Returned populations do not deduct this original outflow again.
    gross.checked_sub(recovered).map_err(|_| corrupt_storage())
}

/// Logical event interpretation across every physical debit. This is distinct
/// from the original group's physical budget and never subtracts returns twice.
pub(in crate::hub) async fn logical_line_net_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &DurableRequestId,
    group: HubRecordId,
    gross: WholePacks,
    cut: HubRevision,
) -> Result<WholePacks, SafeError> {
    let line = Box::pin(super::quantity::original_on(tx, hub, operation, group, cut)).await?;
    if line.original.packs != gross {
        return Err(corrupt_storage());
    }
    let history =
        super::quantity::history_from_original_on(tx, hub, operation, group, cut, &line).await?;
    let additional = additional_debits_on(tx, hub, &line, &history.amendments, cut).await?;
    let mut keys = std::collections::BTreeSet::from([PhysicalDebitKey {
        operation: line.original.operation,
        group,
    }]);
    let zero = WholePacks::from_value(0).expect("zero");
    let mut added = zero;
    for debit in additional {
        if !keys.insert(debit.key) {
            return Err(corrupt_storage());
        }
        added = added
            .checked_add(debit.packs)
            .map_err(|_| corrupt_storage())?;
    }
    let credits = Box::pin(super::return_allowances::credits_on(
        tx,
        hub,
        &line,
        &history.amendments,
    ))
    .await?;
    let mut recovered = zero;
    for credit in credits {
        if !keys.contains(&credit.debit) || credit.stock_revision.value() > cut.value() {
            return Err(corrupt_storage());
        }
        recovered = recovered
            .checked_add(credit.packs)
            .map_err(|_| corrupt_storage())?;
    }
    let account = history.with_verified_returns(zero)?;
    if account.additional_departure() != added || account.found_remainder() != recovered {
        return Err(corrupt_storage());
    }
    Ok(account.effective())
}

/// Purpose-sealed physical outputs, not recursive full predecessor replanning.
/// Current authority first audits complete roots; frozen originals additionally
/// compare every reconstructed fact with immutable native admission material.
pub(super) async fn additional_debits_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    line: &super::quantity::CanonicalLine,
    amendments: &[DepartureQuantityAmendment],
    cut: HubRevision,
) -> Result<Vec<OriginalPhysicalDebit>, SafeError> {
    let mut result = Vec::new();
    for amendment in amendments {
        let DepartureQuantityDelta::AdditionalDeparture(packs) = amendment.delta else {
            continue;
        };
        let operation = crate::hub::actuals::common::operation(amendment.operation)
            .map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind != "correction_additional"
            || op.revision != amendment.stock_revision
            || op.revision.value() > cut.value()
        {
            return Err(corrupt_storage());
        }
        let projection = Box::pin(super::additional_projection::read_on(tx, hub, &op)).await?;
        if projection.quantity.original != *line
            || projection.quantity.amendment != *amendment
            || projection.original.target != line.original
            || projection.original.packs != packs
        {
            return Err(corrupt_storage());
        }
        result.push(projection.original);
    }
    Ok(result)
}

pub(in crate::hub) async fn additional_outflows_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    graph: &harvestcircle_domain::hub::portions::PhysicalLineage,
) -> Result<Vec<(HubRecordId, WholePacks)>, SafeError> {
    // Verify all roots before selecting their canonical population; a damaged
    // denormalized event/trace field cannot hide a debit from its original cut.
    let ids:Vec<String>=sqlx::query_scalar("SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE hub_id=? AND kind='correction_additional' AND stock_revision<=? ORDER BY stock_revision LIMIT 4097")
        .bind(hub.as_bytes().as_slice()).bind(i64::try_from(graph.revision().value()).map_err(|_|corrupt_storage())?)
        .fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() > harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS {
        return Err(corrupt_storage());
    }
    let mut result = Vec::new();
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let projection = Box::pin(super::additional_projection::read_on(tx, hub, &op)).await?;
        if projection.original.before.trace != graph.trace() {
            continue;
        }
        if projection.original.before.variant != graph.variant() {
            return Err(corrupt_storage());
        }
        let target = crate::hub::actuals::common::operation(projection.original.target.operation)
            .map_err(|_| corrupt_storage())?;
        let history = super::quantity::history_on(
            tx,
            hub,
            &target,
            projection.original.target.group,
            graph.revision(),
        )
        .await?;
        let credits = Box::pin(super::return_allowances::credits_on(
            tx,
            hub,
            &projection.quantity.original,
            &history.amendments,
        ))
        .await?;
        let mut recovered = WholePacks::from_value(0).expect("zero");
        for credit in credits {
            if credit.debit == projection.original.key {
                recovered = recovered
                    .checked_add(credit.packs)
                    .map_err(|_| corrupt_storage())?;
            }
        }
        let net = projection
            .original
            .packs
            .checked_sub(recovered)
            .map_err(|_| corrupt_storage())?;
        let node = graph
            .node(projection.original.before.id)
            .ok_or_else(corrupt_storage)?;
        if node.created_revision.value() >= op.revision.value() {
            return Err(corrupt_storage());
        }
        result.push((projection.original.before.id, net));
    }
    Ok(result)
}
