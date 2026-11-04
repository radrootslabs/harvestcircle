//! Canonical issued facts. Mutable closure/status never changes an old source
//! cut, and issued paper is never represented as a physical departure.
use super::*;
use harvestcircle_domain::hub::{
    corrections::AttributionAffectedIssuedVersion,
    dispatch::{HUB_DISPATCH_CAPACITY, HUB_IDENTIFICATION_VERSIONS},
    stock::HUB_STOCK_EVENT_CAPACITY,
};
use std::collections::BTreeSet;

pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<Vec<AttributionAffectedIssuedVersion>, SafeError> {
    load_through_on(tx, hub, None).await
}

pub(super) async fn load_through_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    cut: Option<HubRevision>,
) -> Result<Vec<AttributionAffectedIssuedVersion>, SafeError> {
    let through = cut
        .map(|v| i64::try_from(v.value()).map_err(|_| corrupt_storage()))
        .transpose()?;
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_dispatch_changes c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE (o.operation_id IS NULL OR o.hub_id<>?1 OR (?2 IS NULL OR o.stock_revision<=?2)) AND ",
        "(o.operation_id IS NULL OR o.hub_id<>?1 OR o.result_id<>c.preparation_id OR o.kind NOT IN ('dispatch_prepare','dispatch_revise','dispatch_issue','dispatch_replace','dispatch_unused'))"
    )).bind(hub.as_bytes().as_slice()).bind(through).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let missing:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_dispatch_changes c ON c.operation_id=o.operation_id ",
        "WHERE o.kind IN ('dispatch_prepare','dispatch_revise','dispatch_issue','dispatch_replace','dispatch_unused') AND (? IS NULL OR o.stock_revision<=?) AND c.operation_id IS NULL"
    )).bind(through).bind(through).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let ids:Vec<Vec<u8>>=sqlx::query_scalar("SELECT DISTINCT substr(c.preparation_id,1,17) FROM hub_dispatch_changes c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id WHERE o.operation_id IS NULL OR (? IS NULL OR o.stock_revision<=?) ORDER BY c.preparation_id LIMIT ?")
        .bind(through).bind(through).bind(i64::try_from(HUB_DISPATCH_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 || missing != 0 || ids.len() > HUB_DISPATCH_CAPACITY {
        return Err(corrupt_storage());
    }
    // Every included change has its own complete result seal. A valid later
    // closure must not hide damage in an earlier issue or replacement original.
    let originals=sqlx::query(concat!(
        "SELECT substr(o.operation_id,1,37) AS operation,substr(c.preparation_id,1,17) AS preparation,c.record_revision ",
        "FROM hub_stock_operations o LEFT JOIN hub_dispatch_changes c ON c.operation_id=o.operation_id ",
        "WHERE o.kind IN ('dispatch_prepare','dispatch_revise','dispatch_issue','dispatch_replace','dispatch_unused') ",
        "AND (? IS NULL OR o.stock_revision<=?) ORDER BY o.stock_revision LIMIT ?"
    )).bind(through).bind(through).bind(i64::try_from(HUB_STOCK_EVENT_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if originals.len() > HUB_STOCK_EVENT_CAPACITY {
        return Err(corrupt_storage());
    }
    for original in originals {
        let operation = DurableRequestId::parse(stock::read::text(&original, "operation")?)
            .map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        let preparation = stock::read::record_id(&original, "preparation")?;
        let revision = stock::read::revision_field(&original, "record_revision")?;
        let record = Box::pin(handling::dispatch::read::record_revision_on(
            tx,
            hub,
            preparation,
            revision,
        ))
        .await?;
        if record.id != op.result
            || record.updated.stock_revision != op.revision
            || record.updated.operation
                != crate::hub::actuals::common::operation_id(&operation)
                    .map_err(|_| corrupt_storage())?
        {
            return Err(corrupt_storage());
        }
    }
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for id in ids {
        let id = HubRecordId::from_bytes(id.try_into().map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())?;
        let record = if let Some(through) = through {
            let revision: Option<i64> = sqlx::query_scalar(concat!(
                "SELECT max(c.record_revision) FROM hub_dispatch_changes c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
                "WHERE c.preparation_id=? AND (o.operation_id IS NULL OR o.stock_revision<=?)"
            )).bind(id.as_bytes().as_slice()).bind(through).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
            let revision = stock::read::revision(revision.ok_or_else(corrupt_storage)?)?;
            Box::pin(handling::dispatch::read::record_revision_on(
                tx, hub, id, revision,
            ))
            .await?
        } else {
            Box::pin(handling::dispatch::read::record_on(tx, hub, id)).await?
        };
        if cut.is_some_and(|cut| record.updated.stock_revision.value() > cut.value()) {
            return Err(corrupt_storage());
        }
        for version in record.versions {
            let operation = crate::hub::actuals::common::operation(version.issued.operation)
                .map_err(|_| corrupt_storage())?;
            let op = stock::read::operation_on(tx, hub, &operation)
                .await?
                .ok_or_else(corrupt_storage)?;
            if !seen.insert(version.id)
                || !matches!(op.kind.as_str(), "dispatch_issue" | "dispatch_replace")
                || op.result != record.id
                || op.revision != version.issued.stock_revision
                || op.actor != version.issued.actor
                || op.recorded_at != version.issued.recorded_at
                || version.material.catalog_revision.value() > op.catalog_revision.value()
            {
                return Err(corrupt_storage());
            }
            let populations = version
                .material
                .lines
                .iter()
                .map(|line| line.trace.id)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            result.push(AttributionAffectedIssuedVersion {
                version: version.id,
                stock_revision: op.revision,
                catalog_revision: op.catalog_revision,
                populations,
                recipient: version.material.recipient,
            });
        }
    }
    if result.len() > HUB_DISPATCH_CAPACITY * HUB_IDENTIFICATION_VERSIONS {
        return Err(corrupt_storage());
    }
    result.sort_by_key(|v| (v.stock_revision.value(), v.version));
    Ok(result)
}
