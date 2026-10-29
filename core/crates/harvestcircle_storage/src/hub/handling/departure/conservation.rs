//! Current arithmetic retains actual outflow and immutable event provenance.
use super::super::*;
use crate::db::{corrupt_storage, storage_unavailable};
use crate::hub::stock;
use sqlx::Row;

pub(in crate::hub) async fn validate_headers_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_departures d ON d.operation_id=o.operation_id ",
        "LEFT JOIN hub_dispatch_identifications i ON i.identification_id=d.identification_id LEFT JOIN hub_dispatch_changes paper ON paper.preparation_id=d.preparation_id AND paper.record_revision=d.preparation_revision ",
        "WHERE (o.kind='departure' OR d.operation_id IS NOT NULL) AND (o.hub_id<>? OR o.kind<>'departure' OR d.operation_id IS NULL OR o.result_id<>d.departure_id OR ",
        "i.identification_id IS NULL OR paper.operation_id IS NULL OR paper.kind NOT IN ('dispatch_issue','dispatch_replace') OR paper.identification_id IS NULL OR paper.identification_id<>d.identification_id OR i.preparation_id<>d.preparation_id OR ",
        "EXISTS(SELECT 1 FROM hub_dispatch_changes newer WHERE newer.preparation_id=d.preparation_id AND newer.record_revision>d.preparation_revision) OR d.work_change_count NOT BETWEEN 0 AND 4096 OR d.line_count NOT BETWEEN 1 AND 64 OR (SELECT count(*) FROM hub_departure_lines l WHERE l.operation_id=d.operation_id)<>d.line_count OR ",
        "(SELECT count(*) FROM hub_work_changes w WHERE w.operation_id=d.operation_id)<>d.work_change_count OR ",
        "EXISTS(SELECT 1 FROM hub_departure_lines l WHERE l.operation_id=d.operation_id AND ((SELECT count(*) FROM hub_departure_work_scope s WHERE s.operation_id=l.operation_id AND s.line_ordinal=l.ordinal)<>l.scope_count OR l.ordinal>=d.line_count)))"
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
        "SELECT count(*) AS n,COALESCE(sum(l.packs),0) AS packs,count(CASE WHEN d.operation_id IS NULL OR o.operation_id IS NULL OR i.identification_id IS NULL OR m.operation_id IS NULL OR g.group_id IS NULL OR ",
        "o.hub_id<>? OR o.kind<>'departure' OR o.result_id<>d.departure_id OR i.preparation_id<>d.preparation_id OR ",
        "m.group_id<>l.group_id OR m.trace_id<>l.trace_id OR m.group_revision<>l.before_group_revision OR m.packs<>l.packs OR m.location_id<>l.location_id OR m.location_revision<>l.location_revision OR g.trace_id<>l.trace_id OR ",
        "(SELECT count(*) FROM hub_stock_events e WHERE e.operation_id=l.operation_id AND e.kind='departure' AND e.trace_id=l.trace_id AND e.group_id=l.group_id AND e.packs=l.packs AND ",
        "e.from_location_id=l.location_id AND e.from_location_revision=l.location_revision AND e.to_location_id IS NULL AND e.to_location_revision IS NULL AND e.occurrence_kind=d.occurrence_kind AND e.occurrence_start_unix_s IS d.occurrence_start_unix_s AND e.occurrence_end_unix_s IS d.occurrence_end_unix_s)<>1 ",
        "THEN 1 END) AS invalid FROM hub_departure_lines l LEFT JOIN hub_departures d ON d.operation_id=l.operation_id ",
        "LEFT JOIN hub_stock_operations o ON o.operation_id=d.operation_id LEFT JOIN hub_dispatch_identifications i ON i.identification_id=d.identification_id ",
        "LEFT JOIN hub_dispatch_material_lines m ON m.operation_id=i.material_operation_id AND m.ordinal=l.ordinal LEFT JOIN hub_stock_groups g ON g.group_id=l.group_id WHERE l.trace_id=?"
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
    stock::read::whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)
}
