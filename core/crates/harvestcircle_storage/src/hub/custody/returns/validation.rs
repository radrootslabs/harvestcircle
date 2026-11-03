//! Bounded cross-table structural checks; detailed readers additionally verify seals.
use super::*;
pub(in crate::hub) async fn headers_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_returns returned ON returned.operation_id=o.operation_id LEFT JOIN hub_receipts p ON p.trace_id=returned.population_id ",
        "WHERE (o.kind='return' OR returned.return_id IS NOT NULL) AND (o.hub_id<>? OR o.kind<>'return' OR returned.return_id IS NULL OR p.trace_id IS NULL OR p.operation_id<>o.operation_id OR p.origin_kind<>'return' OR o.result_id<>returned.return_id OR p.supplier_id IS NOT NULL OR p.producer_id IS NOT NULL OR ",
        "returned.source_count<>(SELECT count(*) FROM hub_return_sources s WHERE s.return_id=returned.return_id) OR ",
        "(returned.known_departure_id IS NULL AND returned.source_count<>0) OR ",
        "(returned.known_departure_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM hub_departures d JOIN hub_stock_operations original ON original.operation_id=d.operation_id WHERE d.departure_id=returned.known_departure_id AND original.hub_id=o.hub_id AND original.stock_revision<o.stock_revision AND original.catalog_revision<=o.catalog_revision AND d.line_count=returned.source_count)) OR ",
        "EXISTS(SELECT 1 FROM hub_return_sources s LEFT JOIN hub_departures d ON d.operation_id=s.departure_operation_id LEFT JOIN hub_departure_lines line ON line.operation_id=s.departure_operation_id AND line.ordinal=s.ordinal WHERE s.return_id=returned.return_id AND (s.ordinal>=returned.source_count OR d.departure_id IS NOT returned.known_departure_id OR line.group_id IS NOT s.original_group_id OR s.previously_returned_packs>line.packs)) OR ",
        "(SELECT count(*) FROM hub_work_requirements w WHERE w.created_operation_id=o.operation_id)<>CASE WHEN returned.intake_work_id IS NULL THEN 0 ELSE 1 END OR ",
        "(returned.intake_work_id IS NULL AND p.planned_location_id IS NOT NULL) OR (returned.intake_work_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM hub_work_requirements w WHERE w.work_id=returned.intake_work_id AND w.created_operation_id=o.operation_id AND w.trace_id=p.trace_id AND w.variant_id=p.variant_id AND w.required=p.original_packs AND w.target_location_id=p.planned_location_id AND w.target_location_revision=p.planned_location_revision)) OR ",
        "(SELECT count(*) FROM hub_stock_events e WHERE e.operation_id=o.operation_id AND e.kind='return' AND e.trace_id=p.trace_id AND e.group_id=p.root_group_id AND e.packs=p.original_packs AND e.from_location_id IS NULL AND e.from_location_revision IS NULL AND e.to_location_id=p.receiving_id AND e.to_location_revision=p.receiving_revision AND e.occurrence_kind=p.occurrence_kind AND e.occurrence_start_unix_s IS p.occurrence_start_unix_s AND e.occurrence_end_unix_s IS p.occurrence_end_unix_s)<>1)"
    )).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let orphan:i64=sqlx::query_scalar("SELECT count(*) FROM hub_receipts p LEFT JOIN hub_returns r ON r.population_id=p.trace_id WHERE p.origin_kind='return' AND r.return_id IS NULL").fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 || orphan != 0 {
        return Err(corrupt_storage());
    }
    Ok(())
}
