//! Dispositions are separate outflow; originals, work and physical events must agree.
use super::*;
pub(in crate::hub) async fn headers_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_dispositions d ON d.operation_id=o.operation_id LEFT JOIN hub_stock_groups g ON g.group_id=d.group_id ",
        "WHERE (o.kind='disposition' OR d.operation_id IS NOT NULL) AND (o.hub_id<>? OR o.kind<>'disposition' OR d.operation_id IS NULL OR o.result_id<>d.disposition_id OR g.group_id IS NULL OR g.trace_id<>d.trace_id OR g.revision<>d.after_group_revision OR ",
        "EXISTS(SELECT 1 FROM hub_stock_positions p WHERE p.group_id=d.group_id AND p.packs>0) OR EXISTS(SELECT 1 FROM hub_departure_lines departed WHERE departed.group_id=d.group_id) OR ",
        "d.scope_count<>(SELECT count(*) FROM hub_disposition_work_scope s WHERE s.operation_id=d.operation_id) OR d.work_change_count<>(SELECT count(*) FROM hub_work_changes w WHERE w.operation_id=d.operation_id) OR ",
        "(SELECT count(*) FROM hub_stock_events e WHERE e.operation_id=d.operation_id AND e.kind='disposition' AND e.trace_id=d.trace_id AND e.group_id=d.group_id AND e.packs=d.packs AND e.from_location_id=d.location_id AND e.from_location_revision=d.location_revision AND e.to_location_id IS NULL AND e.to_location_revision IS NULL AND e.occurrence_kind=d.occurrence_kind AND e.occurrence_start_unix_s IS d.occurrence_start_unix_s AND e.occurrence_end_unix_s IS d.occurrence_end_unix_s)<>1)"
    )).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 {
        return Err(corrupt_storage());
    }
    Ok(())
}
pub(in crate::hub) async fn trace_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
) -> Result<WholePacks, SafeError> {
    let row=sqlx::query(concat!(
        "SELECT count(*) AS n,COALESCE(sum(d.packs),0) AS packs,count(CASE WHEN o.operation_id IS NULL OR o.kind<>'disposition' OR o.hub_id<>? OR o.result_id<>d.disposition_id OR g.trace_id<>d.trace_id OR g.revision<>d.after_group_revision OR ",
        "EXISTS(SELECT 1 FROM hub_stock_positions p WHERE p.group_id=d.group_id AND p.packs>0) OR EXISTS(SELECT 1 FROM hub_departure_lines departed WHERE departed.group_id=d.group_id) OR ",
        "(SELECT count(*) FROM hub_stock_events e WHERE e.operation_id=d.operation_id AND e.kind='disposition' AND e.trace_id=d.trace_id AND e.group_id=d.group_id AND e.packs=d.packs AND e.from_location_id=d.location_id AND e.from_location_revision=d.location_revision AND e.to_location_id IS NULL AND e.to_location_revision IS NULL AND e.occurrence_kind=d.occurrence_kind AND e.occurrence_start_unix_s IS d.occurrence_start_unix_s AND e.occurrence_end_unix_s IS d.occurrence_end_unix_s)<>1 THEN 1 END) AS invalid ",
        "FROM hub_dispositions d LEFT JOIN hub_stock_operations o ON o.operation_id=d.operation_id LEFT JOIN hub_stock_groups g ON g.group_id=d.group_id WHERE d.trace_id=?"
    )).bind(hub.as_bytes().as_slice()).bind(trace.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let n: i64 = row.try_get("n").map_err(|_| corrupt_storage())?;
    if !(0..=harvestcircle_domain::hub::portions::HUB_LINEAGE_GROUPS as i64).contains(&n)
        || row
            .try_get::<i64, _>("invalid")
            .map_err(|_| corrupt_storage())?
            != 0
    {
        return Err(corrupt_storage());
    }
    whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)
}
