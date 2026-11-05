//! Count classification never borrows a physical debit's return allowance.
//! Every return through the requested cut is authenticated before narrowing
//! scope. Unsupported count claims remain uncertainty, never zero clearance.
use super::*;
use harvestcircle_domain::hub::{
    corrections::{CountExplanationReturns, count_explanation_id},
    returns::ReturnLinkResolution,
    stock::HUB_TRACE_CAPACITY,
};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

pub(super) struct Returns {
    pub totals: Vec<CountExplanationReturns>,
    pub unresolved: bool,
}
pub(super) async fn for_history_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    history: &super::count_account::History,
    cut: HubRevision,
) -> Result<Returns, SafeError> {
    let operations = history
        .explanations
        .iter()
        .map(|e| e.operation)
        .collect::<BTreeSet<_>>();
    let records = operations
        .iter()
        .map(|op| {
            count_explanation_id(*op)
                .map(|id| (id, *op))
                .map_err(|_| corrupt_storage())
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let through = i64::try_from(cut.value()).map_err(|_| corrupt_storage())?;
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_returns r LEFT JOIN hub_stock_operations o ON o.operation_id=r.operation_id ",
        "WHERE (o.operation_id IS NULL OR o.hub_id<>? OR o.stock_revision<=?) ",
        "AND (o.operation_id IS NULL OR o.hub_id<>? OR o.kind<>'return')"
    )).bind(hub.as_bytes().as_slice()).bind(through).bind(hub.as_bytes().as_slice())
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    // Start from canonical operations so a missing body cannot remove an
    // unresolved return before authentication or manufacture a zero floor.
    let ids:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(o.operation_id,1,37) FROM hub_stock_operations o ",
        "WHERE o.kind='return' AND o.hub_id=? AND o.stock_revision<=? ORDER BY o.stock_revision LIMIT 4097"
    )).bind(hub.as_bytes().as_slice()).bind(through)
        .fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() > HUB_TRACE_CAPACITY {
        return Err(stock::capacity());
    }
    let mut result = Returns {
        totals: Vec::new(),
        unresolved: false,
    };
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind != "return" || op.revision.value() > cut.value() {
            return Err(corrupt_storage());
        }
        let (variant, claim, known, established) = if let Some(projected) =
            super::return_projection::read_on(tx, hub, &op).await?
        {
            if projected.stock_revision != op.revision || projected.recorded_at != op.recorded_at {
                return Err(corrupt_storage());
            }
            (
                projected.variant,
                Some(projected.claim_departure),
                Some(projected.known_operation),
                projected.established_group.is_some(),
            )
        } else {
            // Only exact legacy None capture and projection absence may select
            // this existing full-original path. It verifies the preserved raw
            // claim, restricted intake and original normal-departure evidence.
            let original = Box::pin(crate::hub::custody::returns::read::legacy_original_on(
                tx, hub, op,
            ))
            .await?
            .original;
            (
                original.intake.variant.id,
                original.linkage.claim.map(|claim| claim.departure),
                original
                    .linkage
                    .departure
                    .as_ref()
                    .map(|known| known.operation),
                original.linkage.resolution == ReturnLinkResolution::EstablishedScope,
            )
        };
        let related = claim.is_some_and(|id| records.contains_key(&id))
            || known.is_some_and(|op| operations.contains(&op));
        if variant != history.original.variant && !related {
            continue;
        }
        if !established {
            result.unresolved = true;
            continue;
        }
        if related {
            // The retained normal return schema establishes dispatch departures
            // only. A count-associated established row requires its own fully
            // qualified native representation; it cannot be accepted through
            // a normal physical-return projection or fabricated zero floor.
            return Err(corrupt_storage());
        }
        // A verified established normal scope is disjoint from this count.
    }
    Ok(result)
}
