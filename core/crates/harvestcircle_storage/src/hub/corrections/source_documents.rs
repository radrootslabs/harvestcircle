//! Canonical issued facts. Mutable closure/status never changes an old source
//! cut, and issued paper is never represented as a physical departure.
use super::*;
use harvestcircle_domain::hub::{
    corrections::AttributionAffectedIssuedVersion,
    dispatch::{HUB_DISPATCH_CAPACITY, HUB_IDENTIFICATION_VERSIONS},
};
use std::collections::BTreeSet;

pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<Vec<AttributionAffectedIssuedVersion>, SafeError> {
    let missing:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_dispatch_changes c ON c.operation_id=o.operation_id ",
        "WHERE o.kind IN ('dispatch_prepare','dispatch_revise','dispatch_issue','dispatch_replace','dispatch_unused') AND c.operation_id IS NULL"
    )).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let ids:Vec<Vec<u8>>=sqlx::query_scalar("SELECT DISTINCT substr(preparation_id,1,17) FROM hub_dispatch_changes ORDER BY preparation_id LIMIT ?")
        .bind(i64::try_from(HUB_DISPATCH_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if missing != 0 || ids.len() > HUB_DISPATCH_CAPACITY {
        return Err(corrupt_storage());
    }
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for id in ids {
        let id = HubRecordId::from_bytes(id.try_into().map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())?;
        let record = Box::pin(handling::dispatch::read::record_on(tx, hub, id)).await?;
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
