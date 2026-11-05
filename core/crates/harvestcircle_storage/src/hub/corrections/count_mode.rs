//! Count-aware interpretation is selected by immutable native activation and
//! operation records, including an empty inventory. JSON never selects a mode.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::hub) struct Epoch {
    pub stock: u64,
    pub catalog: u64,
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::hub) struct Mode {
    pub epoch: Epoch,
    pub capture: [u8; 32],
}
pub(in crate::hub) async fn epoch_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<Epoch, SafeError> {
    // No native count root is legitimate in a physical-only published producer.
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 21 {
        return Err(corrupt_storage());
    }
    let row=sqlx::query(concat!(
        "SELECT e.epoch,e.legacy_stock_revision,e.legacy_catalog_revision,s.revision,old.epoch AS old_epoch,",
        "old.legacy_stock_revision AS old_stock,old.legacy_catalog_revision AS old_catalog,",
        "(SELECT count(*) FROM hub_actual_interpretation_epochs) AS old_total,",
        "COALESCE((SELECT max(o.catalog_revision) FROM hub_stock_operations o WHERE o.hub_id=e.hub_id AND o.stock_revision<=old.legacy_stock_revision),1) AS old_dependency,",
        "(SELECT count(*) FROM hub_count_interpretation_epochs) AS total,",
        "COALESCE((SELECT max(v.catalog_revision) FROM hub_reference_versions v WHERE v.hub_id=e.hub_id),1) AS catalog,",
        "COALESCE((SELECT max(o.catalog_revision) FROM hub_stock_operations o WHERE o.hub_id=e.hub_id AND o.stock_revision<=e.legacy_stock_revision),1) AS dependency ",
        "FROM hub_count_interpretation_epochs e JOIN hub_stock_state s ON s.hub_id=e.hub_id ",
        "JOIN hub_actual_interpretation_epochs old ON old.hub_id=e.hub_id ",
        "JOIN hub_context h ON h.hub_id=e.hub_id WHERE e.hub_id=? LIMIT 1"
    )).bind(hub.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let number = |name| -> Result<u64, SafeError> {
        u64::try_from(row.try_get::<i64, _>(name).map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())
    };
    let epoch = Epoch {
        stock: number("legacy_stock_revision")?,
        catalog: number("legacy_catalog_revision")?,
    };
    if number("epoch")? != 1
        || number("total")? != 1
        || epoch.stock == 0
        || epoch.catalog == 0
        || epoch.stock > number("revision")?
        || epoch.catalog > number("catalog")?
        || number("dependency")? > epoch.catalog
        || number("old_epoch")? != 1
        || number("old_stock")? == 0
        || number("old_catalog")? == 0
        || number("old_stock")? > epoch.stock
        || number("old_catalog")? > epoch.catalog
        || number("old_total")? != 1
        || number("old_dependency")? > number("old_catalog")?
    {
        return Err(corrupt_storage());
    }
    Ok(epoch)
}
pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<Mode, SafeError> {
    let epoch = epoch_on(tx, hub).await?;
    if !matches!(
        op.kind.as_str(),
        "actual_review" | "correction_count_explain" | "correction_count_classify"
    ) || op.revision.value() <= epoch.stock
        || op.catalog_revision.value() < epoch.catalog
    {
        return Err(corrupt_storage());
    }
    let row = sqlx::query(concat!(
        "SELECT substr(hub_id,1,17) AS hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,",
        "stock_revision,catalog_revision,substr(capture_hash,1,33) AS capture_hash ",
        "FROM hub_count_interpretation_markers WHERE operation_id=? LIMIT 1"
    ))
    .bind(op.id.as_str())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?
    .ok_or_else(corrupt_storage)?;
    let number = |name| -> Result<u64, SafeError> {
        u64::try_from(row.try_get::<i64, _>(name).map_err(|_| corrupt_storage())?)
            .map_err(|_| corrupt_storage())
    };
    let owner: Vec<u8> = row.try_get("hub_id").map_err(|_| corrupt_storage())?;
    let capture: Vec<u8> = row.try_get("capture_hash").map_err(|_| corrupt_storage())?;
    if owner.as_slice() != hub.as_bytes()
        || number("epoch")? != 1
        || number("legacy_stock_revision")? != epoch.stock
        || number("legacy_catalog_revision")? != epoch.catalog
        || number("stock_revision")? != op.revision.value()
        || number("catalog_revision")? != op.catalog_revision.value()
    {
        return Err(corrupt_storage());
    }
    Ok(Mode {
        epoch,
        capture: capture.try_into().map_err(|_| corrupt_storage())?,
    })
}

pub(in crate::hub) async fn for_actual_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<Option<Mode>, SafeError> {
    if op.kind != "actual_review" {
        return Err(corrupt_storage());
    }
    let epoch = epoch_on(tx, hub).await?;
    if op.revision.value() > epoch.stock {
        return original_on(tx, hub, op).await.map(Some);
    }
    let markers: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hub_count_interpretation_markers WHERE operation_id=?",
    )
    .bind(op.id.as_str())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if markers != 0 {
        return Err(corrupt_storage());
    }
    Ok(None)
}
pub(in crate::hub) async fn persist_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<(), SafeError> {
    let epoch = epoch_on(tx, hub).await?;
    if !matches!(
        op.kind.as_str(),
        "actual_review" | "correction_count_explain" | "correction_count_classify"
    ) || op.revision.value() <= epoch.stock
        || op.catalog_revision.value() < epoch.catalog
    {
        return Err(corrupt_storage());
    }
    let changed=sqlx::query(concat!(
        "INSERT INTO hub_count_interpretation_markers (operation_id,hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,stock_revision,catalog_revision,capture_hash) ",
        "SELECT ?,?,1,?,?,?,?,s.capture_hash FROM hub_submissions s WHERE s.operation_id=?"
    )).bind(op.id.as_str()).bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(epoch.stock).map_err(|_|corrupt_storage())?).bind(i64::try_from(epoch.catalog).map_err(|_|corrupt_storage())?)
        .bind(i64::try_from(op.revision.value()).map_err(|_|corrupt_storage())?).bind(i64::try_from(op.catalog_revision.value()).map_err(|_|corrupt_storage())?)
        .bind(op.id.as_str()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(corrupt_storage());
    }
    Ok(())
}
pub(in crate::hub) async fn audit_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let epoch = epoch_on(tx, hub).await?;
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_count_interpretation_markers m ON m.operation_id=o.operation_id ",
        "WHERE o.kind IN ('actual_review','correction_count_explain','correction_count_classify') AND ",
        "((o.stock_revision>?2 AND m.operation_id IS NULL) OR (o.stock_revision<=?2 AND (m.operation_id IS NOT NULL OR o.kind<>'actual_review')))) + ",
        "(SELECT count(*) FROM hub_count_interpretation_markers m LEFT JOIN hub_stock_operations o ON o.operation_id=m.operation_id ",
        "LEFT JOIN hub_actual_reviews r ON r.operation_id=m.operation_id LEFT JOIN hub_corrections c ON c.operation_id=m.operation_id ",
        "LEFT JOIN hub_submissions s ON s.operation_id=m.operation_id ",
        "WHERE o.operation_id IS NULL OR s.operation_id IS NULL OR o.kind NOT IN ('actual_review','correction_count_explain','correction_count_classify') OR ",
        "m.hub_id<>?1 OR o.hub_id<>m.hub_id OR s.hub_id<>m.hub_id OR s.kind<>o.kind OR s.actor_public_key<>o.actor_public_key OR s.domain_intent_hash<>o.intent_hash OR ",
        "m.epoch<>1 OR m.legacy_stock_revision<>?2 OR m.legacy_catalog_revision<>?3 OR o.stock_revision<=?2 OR ",
        "m.stock_revision<>o.stock_revision OR m.catalog_revision<>o.catalog_revision OR m.catalog_revision<?3 OR m.capture_hash<>s.capture_hash OR length(m.capture_hash)<>32 OR ",
        "(o.kind='actual_review' AND r.operation_id IS NULL) OR ",
        "(o.kind='correction_count_explain' AND (c.family IS NULL OR c.family<>'count_explain' OR NOT EXISTS(SELECT 1 FROM hub_correction_count_explanations p WHERE p.operation_id=o.operation_id))) OR ",
        "(o.kind='correction_count_classify' AND (c.family IS NULL OR c.family<>'count_classify' OR NOT EXISTS(SELECT 1 FROM hub_correction_quantity_changes p WHERE p.operation_id=o.operation_id AND p.delta_kind='classification'))))"
    )).bind(hub.as_bytes().as_slice()).bind(i64::try_from(epoch.stock).map_err(|_|corrupt_storage())?)
        .bind(i64::try_from(epoch.catalog).map_err(|_|corrupt_storage())?).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    Ok(())
}
