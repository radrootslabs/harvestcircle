//! Atomic count reconciliation and immutable originals, distinct from observations.
use super::*;
use harvestcircle_domain::hub::{
    counts::{
        CountEffect, CountPostCommand, CountPostPlan, CountPostReferences, CountSurplusIdentities,
        CountSurplusPlan, CountWorkChange, count_post_result_digest,
    },
    movement::{
        PhysicalIdentification, PhysicalStockGroup, WorkAllocation, WorkProgress, WorkRequirement,
    },
    reference::{HubLabel, SourceAttribution, WholePacks},
    stock::{Occurrence, QuantityWork},
    trace::TraceCountLoss,
    treatment::AcceptanceStatus,
};
mod read;
#[cfg(test)]
mod tests;
mod write;
pub(in crate::hub) use read::{losses_on, original_on};

pub(in crate::hub) struct RemovedWork {
    pub total: WholePacks,
    pub completed: WholePacks,
    pub retired: WholePacks,
}

pub(in crate::hub) async fn loss_quantity_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
) -> Result<WholePacks, SafeError> {
    let zero = WholePacks::from_value(0).map_err(|_| corrupt_storage())?;
    losses_on(tx, hub, trace)
        .await?
        .iter()
        .try_fold(zero, |sum, loss| {
            sum.checked_add(loss.packs).map_err(|_| corrupt_storage())
        })
}

pub(in crate::hub) async fn removed_work_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    work: HubRecordId,
) -> Result<RemovedWork, SafeError> {
    let operations:Vec<String>=sqlx::query_scalar(concat!("SELECT substr(r.operation_id,1,37) FROM hub_count_reconciliation_work w JOIN hub_count_reconciliations r ON r.reconciliation_id=w.reconciliation_id ",
        "JOIN hub_stock_operations o ON o.operation_id=r.operation_id WHERE w.work_id=? AND w.removed_packs>0 AND o.hub_id=? ORDER BY o.stock_revision LIMIT 4097"))
        .bind(work.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if operations.len() > HUB_COUNT_CAPACITY {
        return Err(corrupt_storage());
    }
    let zero = WholePacks::from_value(0).map_err(|_| corrupt_storage())?;
    let mut total = RemovedWork {
        total: zero,
        completed: zero,
        retired: zero,
    };
    for id in operations {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let plan = original_on(tx, hub, op).await?.original;
        let scope = plan
            .work_scope_changes()
            .map_err(|_| corrupt_storage())?
            .into_iter()
            .find(|s| s.work == work)
            .ok_or_else(corrupt_storage)?;
        total.total = total
            .total
            .checked_add(scope.removed)
            .map_err(|_| corrupt_storage())?;
        match scope.removed_progress {
            WorkProgress::Completed => {
                total.completed = total
                    .completed
                    .checked_add(scope.removed)
                    .map_err(|_| corrupt_storage())?
            }
            WorkProgress::NoLongerRequired => {
                total.retired = total
                    .retired
                    .checked_add(scope.removed)
                    .map_err(|_| corrupt_storage())?
            }
            WorkProgress::Pending => return Err(corrupt_storage()),
        }
    }
    Ok(total)
}

pub(in crate::hub) async fn validate_headers_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_count_reconciliations r ON r.operation_id=o.operation_id ",
        "LEFT JOIN hub_count_observations c ON c.count_id=r.count_id LEFT JOIN hub_stock_operations observed ON observed.operation_id=c.operation_id ",
        "WHERE (o.kind='count_post' OR r.operation_id IS NOT NULL) AND (o.hub_id<>? OR o.kind<>'count_post' OR r.operation_id IS NULL OR o.result_id<>r.reconciliation_id OR ",
        "c.count_id IS NULL OR observed.operation_id IS NULL OR observed.kind<>'count_observe' OR observed.hub_id<>o.hub_id OR observed.stock_revision>=o.stock_revision OR observed.recorded_at_unix_s>o.recorded_at_unix_s OR ",
        "r.policy_version<>1 OR r.scope_work_count NOT BETWEEN 0 AND 64 OR (SELECT count(*) FROM hub_count_reconciliation_work w WHERE w.reconciliation_id=r.reconciliation_id)<>r.scope_work_count OR ",
        "EXISTS(SELECT 1 FROM hub_count_reconciliation_work w WHERE w.reconciliation_id=r.reconciliation_id AND w.ordinal>=r.scope_work_count))"
    )).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    Ok(())
}

pub(in crate::hub) async fn require_not_counted_empty_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    group: HubRecordId,
    catalog: &harvestcircle_domain::hub::reference::ReferenceCatalog,
) -> Result<(), SafeError> {
    let id:Option<String>=sqlx::query_scalar(concat!(
        "SELECT substr(r.operation_id,1,37) FROM hub_count_reconciliations r JOIN hub_count_observations c ON c.count_id=r.count_id JOIN hub_stock_operations o ON o.operation_id=r.operation_id ",
        "WHERE c.group_id=? AND r.effect_kind='shortage' AND r.after_on_hand_packs=0 AND (SELECT COALESCE(sum(p.packs),0) FROM hub_stock_positions p WHERE p.group_id=c.group_id)=0 ORDER BY o.stock_revision DESC LIMIT 1"
    )).bind(group.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let Some(id) = id else {
        return Ok(());
    };
    let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
    let op = stock::read::operation_on(tx, hub, &id)
        .await?
        .ok_or_else(corrupt_storage)?;
    let plan = original_on(tx, hub, op).await?.original;
    let CountEffect::Shortage {
        after_packs,
        after_revision,
        ..
    } = plan.effect
    else {
        return Err(corrupt_storage());
    };
    if after_packs.value() != 0 {
        return Err(corrupt_storage());
    }
    let state = stock::read::state_on(tx, hub).await?;
    let lineage =
        handling::lineage::load_on(tx, hub, plan.before.trace, catalog, state.revision).await?;
    let row = lineage
        .rows
        .iter()
        .find(|r| r.id == group)
        .ok_or_else(corrupt_storage)?;
    if row.packs.value() != 0 || row.revision != after_revision {
        return Err(corrupt_storage());
    }
    Err(stock::conflict())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountPostReceipt {
    pub operation: DurableRequestId,
    pub original: CountPostPlan,
    /// As created; later movement/treatment never rewrites this original.
    pub surplus: Option<CountSurplusPlan>,
}

impl Database {
    pub async fn apply_hub_count_post(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: CountPostCommand,
        now: UnixTimestamp,
    ) -> Result<HubCountPostReceipt, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    let hash = command.fingerprint(hub.id(), actor, operation_id(&operation)?);
                    if let Some(original) =
                        stock::read::operation_on(tx, hub.id(), &operation).await?
                    {
                        if original.kind != "count_post"
                            || original.actor != actor
                            || original.hash != hash
                        {
                            return Err(stock::conflict());
                        }
                        return original_on(tx, hub.id(), original).await;
                    }
                    write::apply_on(tx, hub.id(), actor, operation, command, hash, now).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    pub async fn hub_count_post_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubCountPostReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    match stock::read::operation_on(tx, hub.id(), &operation).await? {
                        Some(original) => Ok(Some(original_on(tx, hub.id(), original).await?)),
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

fn integer(value: HubRevision) -> Result<i64, SafeError> {
    i64::try_from(value.value()).map_err(|_| corrupt_storage())
}
fn progress(value: WorkProgress) -> &'static str {
    match value {
        WorkProgress::Pending => "pending",
        WorkProgress::Completed => "completed",
        WorkProgress::NoLongerRequired => "no_longer_required",
    }
}
fn decode_progress(value: &str) -> Result<WorkProgress, SafeError> {
    match value {
        "pending" => Ok(WorkProgress::Pending),
        "completed" => Ok(WorkProgress::Completed),
        "no_longer_required" => Ok(WorkProgress::NoLongerRequired),
        _ => Err(corrupt_storage()),
    }
}
