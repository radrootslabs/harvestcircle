//! Bounded physical summaries, separately sealed from full review originals.
//! Prior activity/conservation/duplicate reads never recursively replan reviews.
use super::{common::*, reports};
use crate::{
    db::{corrupt_storage, storage_unavailable},
    hub::{handling, reference, stock},
};
use harvestcircle_domain::{
    SafeError,
    hub::{
        HubId,
        actuals::{
            ActualKnownDeparture, ActualKnownDepartureLine, actual_departure_projection_digest,
        },
    },
};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;
use stock::read::{OperationRecord, record_id, revision_field, whole_packs};

pub(in crate::hub) async fn known_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<ActualKnownDeparture, SafeError> {
    if op.kind != "actual_review" {
        return Err(stock::conflict());
    }
    let row=sqlx::query(concat!(
        "SELECT substr(report_id,1,17) AS report_id,policy_version,substr(review_kind,1,33) AS review_kind,",
        "substr(trace_id,1,17) AS trace_id,substr(group_id,1,17) AS group_id,substr(variant_id,1,17) AS variant_id,variant_revision,",
        "substr(recipient_id,1,17) AS recipient_id,recipient_revision,packs,substr(source_operation_id,1,37) AS source_operation,",
        "substr(source_group_id,1,17) AS source_group,substr(historical_location_id,1,17) AS historical_location,historical_location_revision,",
        "occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,substr(projection_hash,1,33) AS projection_hash ",
        "FROM hub_actual_reviews WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if row
        .try_get::<String, _>("review_kind")
        .map_err(|_| corrupt_storage())?
        != "departure"
    {
        return Err(stock::invalid_input());
    }
    if row
        .try_get::<i64, _>("policy_version")
        .map_err(|_| corrupt_storage())?
        != 1
        || record_id(&row, "report_id")? != op.result
    {
        return Err(corrupt_storage());
    }
    let report = reports::by_id_on(tx, hub, op.result).await?.original;
    if report.stock_revision.value() >= op.revision.value() || report.recorded_at > op.recorded_at {
        return Err(corrupt_storage());
    }
    let trace = record_id(&row, "trace_id")?;
    let group = record_id(&row, "group_id")?;
    let population = handling::read::trace_on(tx, hub, trace).await?;
    let variant = reference::record_at_catalog_on(
        tx,
        hub,
        record_id(&row, "variant_id")?,
        op.catalog_revision,
    )
    .await?;
    let recipient = reference::record_at_catalog_on(
        tx,
        hub,
        record_id(&row, "recipient_id")?,
        op.catalog_revision,
    )
    .await?;
    if variant.id != population.variant.id
        || variant.revision != revision_field(&row, "variant_revision")?
        || recipient.revision != revision_field(&row, "recipient_revision")?
    {
        return Err(corrupt_storage());
    }
    let source_operation = harvestcircle_application::DurableRequestId::parse(
        row.try_get::<String, _>("source_operation")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let source = handling::actuals::source_original_on(
        tx,
        hub,
        &population,
        &source_operation,
        record_id(&row, "source_group")?,
    )
    .await?;
    if source.location.id != record_id(&row, "historical_location")?
        || source.location.revision != revision_field(&row, "historical_location_revision")?
    {
        return Err(corrupt_storage());
    }
    let member:i64=sqlx::query_scalar("SELECT count(*) FROM hub_stock_groups g JOIN hub_group_identification i ON i.group_id=g.group_id JOIN hub_stock_operations birth ON birth.operation_id=i.created_operation_id WHERE g.group_id=? AND g.trace_id=? AND birth.hub_id=? AND birth.stock_revision<?")
        .bind(group.as_bytes().as_slice()).bind(trace.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).bind(integer(op.revision)?)
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if member != 1 {
        return Err(corrupt_storage());
    }
    let known = ActualKnownDeparture {
        operation: operation_id(&op.id).map_err(|_| corrupt_storage())?,
        actor: op.actor,
        hub,
        stock_revision: op.revision,
        catalog_revision: op.catalog_revision,
        trace,
        variant,
        recipient,
        occurred: stock::read::occurrence(&row)?,
        recorded_at: op.recorded_at,
        lines: vec![ActualKnownDepartureLine {
            group,
            location: source.location.clone(),
            packs: whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?,
        }],
    };
    let saved: Vec<u8> = row
        .try_get("projection_hash")
        .map_err(|_| corrupt_storage())?;
    if actual_departure_projection_digest(&op.hash, report.id, &source, &known)
        .map_err(|_| corrupt_storage())?
        .as_slice()
        != saved.as_slice()
    {
        return Err(corrupt_storage());
    }
    let event:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_events e WHERE e.operation_id=? AND e.kind='actual_review' AND e.trace_id=? AND e.group_id=? AND e.packs=? ",
        "AND e.from_location_id=? AND e.from_location_revision=? AND e.to_location_id IS NULL AND e.to_location_revision IS NULL ",
        "AND e.occurrence_kind=? AND e.occurrence_start_unix_s IS ? AND e.occurrence_end_unix_s IS ?"
    )).bind(op.id.as_str()).bind(trace.as_bytes().as_slice()).bind(group.as_bytes().as_slice()).bind(i64::from(known.lines[0].packs.value()))
        .bind(source.location.id.as_bytes().as_slice()).bind(integer(source.location.revision)?)
        .bind(occurrence_fields(known.occurred).0).bind(occurrence_fields(known.occurred).1).bind(occurrence_fields(known.occurred).2)
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_stock_events WHERE operation_id=?")
            .bind(op.id.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
    let closed:i64=sqlx::query_scalar("SELECT count(*) FROM hub_actual_report_reviews WHERE report_id=? AND operation_id=? AND review_operation_id=?")
        .bind(report.id.as_bytes().as_slice()).bind(operation(report.operation).map_err(|_|corrupt_storage())?.as_str()).bind(op.id.as_str())
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if event != 1 || total != 1 || closed != 1 {
        return Err(corrupt_storage());
    }
    Ok(known)
}
