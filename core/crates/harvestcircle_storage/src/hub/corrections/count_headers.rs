//! Complete canonical count roots. No denormalized scope may hide a final
//! explanation/classification or turn missing evidence into an empty account.
use super::*;

pub(super) async fn on(tx: &mut OriginalReader<'_, '_>, hub: HubId) -> Result<(), SafeError> {
    let roots:i64=sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_stock_operations WHERE kind IN ('correction_count_explain','correction_count_classify'))+",
        "(SELECT count(*) FROM hub_corrections WHERE family IN ('count_explain','count_classify'))+",
        "(SELECT count(*) FROM hub_correction_count_explanations)+",
        "(SELECT count(*) FROM hub_correction_quantity_changes WHERE delta_kind='classification')"
    )).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if crate::HARVESTCIRCLE_STATE_SCHEMA_VERSION < 21 {
        return if roots == 0 {
            Ok(())
        } else {
            Err(corrupt_storage())
        };
    }
    super::count_mode::audit_on(tx, hub).await?;
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_corrections c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE c.family IN ('count_explain','count_classify') AND (o.operation_id IS NULL OR o.hub_id<>?1 OR o.result_id<>c.correction_id OR ",
        "o.kind<>CASE c.family WHEN 'count_explain' THEN 'correction_count_explain' ELSE 'correction_count_classify' END)) + ",
        "(SELECT count(*) FROM hub_correction_count_explanations p LEFT JOIN hub_corrections c ON c.operation_id=p.operation_id ",
        "WHERE c.operation_id IS NULL OR c.family<>'count_explain') + ",
        "(SELECT count(*) FROM hub_correction_quantity_changes p LEFT JOIN hub_corrections c ON c.operation_id=p.operation_id ",
        "WHERE p.delta_kind='classification' AND (c.operation_id IS NULL OR c.family<>'count_classify')) + ",
        "(SELECT count(*) FROM hub_stock_operations o WHERE o.kind IN ('correction_count_explain','correction_count_classify') ",
        "AND EXISTS(SELECT 1 FROM hub_stock_events e WHERE e.operation_id=o.operation_id))"
    )).bind(hub.as_bytes().as_slice()).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    let ids:Vec<String>=sqlx::query_scalar("SELECT operation_id FROM hub_stock_operations WHERE kind IN ('correction_count_explain','correction_count_classify') ORDER BY stock_revision LIMIT ?")
        .bind(i64::try_from(harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS+1).map_err(|_|corrupt_storage())?)
        .fetch_all(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() > harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS {
        return Err(corrupt_storage());
    }
    let checkpoint = tx.count_prefix_checkpoint();
    match verify_operations_on(tx, hub, ids, None).await {
        Ok(()) => Ok(()),
        Err(error) => {
            tx.restore_count_prefix(checkpoint);
            Err(error)
        }
    }
}

// Authenticate the complete historical prefix in journal order before selected
// or per-account assembly. An E/C predecessor must be proved before a later
// original asks for it, including when their count accounts are interleaved.
pub(super) async fn through_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    cut: HubRevision,
) -> Result<(), SafeError> {
    if tx.count_prefix_covers(hub, cut).await? {
        return Ok(());
    }
    let checkpoint = tx.count_prefix_checkpoint();
    let ids: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT operation_id FROM hub_stock_operations WHERE hub_id=? AND stock_revision<=? ",
        "AND kind IN ('correction_count_explain','correction_count_classify') ORDER BY stock_revision LIMIT ?"
    ))
    .bind(hub.as_bytes().as_slice())
    .bind(i64::try_from(cut.value()).map_err(|_| corrupt_storage())?)
    .bind(i64::try_from(harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS + 1).map_err(|_| corrupt_storage())?)
    .fetch_all(&mut **tx).await.map_err(|_| storage_unavailable())?;
    if ids.len() > harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS {
        return Err(corrupt_storage());
    }
    match verify_operations_on(tx, hub, ids, Some(cut)).await {
        Ok(()) => {
            tx.complete_count_prefix(hub, cut);
            Ok(())
        }
        Err(error) => {
            tx.restore_count_prefix(checkpoint);
            Err(error)
        }
    }
}

async fn verify_operations_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    ids: Vec<String>,
    cut: Option<HubRevision>,
) -> Result<(), SafeError> {
    if !ids.is_empty() {
        tx.require_count_read_phase().await?;
    }
    let mut prior = None;
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if cut.is_some_and(|cut| op.revision.value() > cut.value())
            || prior.is_some_and(|prior| op.revision.value() <= prior)
            || !matches!(
                op.kind.as_str(),
                "correction_count_explain" | "correction_count_classify"
            )
        {
            return Err(corrupt_storage());
        }
        prior = Some(op.revision.value());
        // Both callers supply the complete SQL-selected chronological list.
        // Every preceding root is now fully proved; the intervening journal
        // gap contains no unprocessed E/C root. Exclude this unproved original.
        let before = HubRevision::from_value(
            op.revision
                .value()
                .checked_sub(1)
                .ok_or_else(corrupt_storage)?,
        )
        .map_err(|_| corrupt_storage())?;
        tx.complete_count_prefix(hub, before);
        match op.kind.as_str() {
            "correction_count_explain" => Box::pin(tx.verify_count_explanation(hub, &op)).await?,
            "correction_count_classify" => {
                Box::pin(tx.verify_count_classification(hub, &op)).await?
            }
            _ => return Err(corrupt_storage()),
        }
        tx.complete_count_prefix(hub, op.revision);
    }
    Ok(())
}
