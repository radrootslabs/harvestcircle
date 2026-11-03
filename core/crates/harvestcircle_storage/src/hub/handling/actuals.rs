//! Canonical physical originals for actual-report review, never an effect grant.
use super::super::stock;
use super::*;
use crate::db::corrupt_storage;
use harvestcircle_domain::hub::actuals::{
    ActualKnownDeparture, ActualKnownDepartureLine, ActualPhysicalActivity,
    ActualPhysicalActivityKind as Activity, HistoricalActualPosition, HistoricalActualSource,
};
use harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY;
use sqlx::Row;
use stock::read::{record_id, revision_field};

fn operation_id(operation: &DurableRequestId) -> Result<HubRecordId, SafeError> {
    HubRecordId::from_hex(&operation.as_str().replace('-', "")).map_err(|_| corrupt_storage())
}

pub(in crate::hub) async fn source_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    lineage: &lineage::LoadedLineage,
    operation: &DurableRequestId,
    group: HubRecordId,
) -> Result<HistoricalActualSource, SafeError> {
    let op = stock::read::operation_on(tx, hub, operation)
        .await?
        .ok_or_else(invalid_input)?;
    let revision = op.revision;
    let catalog_revision = op.catalog_revision;
    let recorded_at = op.recorded_at;
    let node = lineage.graph.node(group).ok_or_else(invalid_input)?;
    let (location, packs, group_revision, occurred) = match op.kind.as_str() {
        "receipt" | "opening" | "count_post" | "return"
            if node.parent.is_none() && node.created_revision == revision =>
        {
            let population = stock::read::population_on(tx, hub, op, lineage.trace.id).await?;
            let row = lineage
                .rows
                .iter()
                .find(|r| r.id == group)
                .ok_or_else(corrupt_storage)?;
            if population != lineage.trace || &row.created_operation != operation {
                return Err(invalid_input());
            }
            (
                population.initial_location,
                population.original_packs,
                HubRevision::INITIAL,
                population.occurred,
            )
        }
        "move" => {
            let original = history::movement_on(tx, hub, op).await?;
            let HubHandlingResult::Moved(moved) = original.result else {
                return Err(corrupt_storage());
            };
            if original.trace != lineage.trace.id || moved.moved_group != group {
                return Err(invalid_input());
            }
            let group_revision = if moved.separated_portion.is_some() {
                HubRevision::INITIAL
            } else {
                HubRevision::from_value(
                    moved
                        .expected_group_revision
                        .value()
                        .checked_add(1)
                        .ok_or_else(corrupt_storage)?,
                )
                .map_err(|_| corrupt_storage())?
            };
            (moved.to, moved.packs, group_revision, moved.occurred)
        }
        "split" => {
            let original = portions::original_on(tx, hub, op).await?;
            let HubHandlingResult::Separated(split) = original.result else {
                return Err(corrupt_storage());
            };
            if original.trace != lineage.trace.id || split.portion_group != group {
                return Err(invalid_input());
            }
            (
                split.location,
                split.packs,
                HubRevision::INITIAL,
                split.occurred,
            )
        }
        _ => return Err(invalid_input()),
    };
    let source = HistoricalActualSource {
        operation: operation_id(operation)?,
        hub,
        stock_revision: revision,
        catalog_revision,
        group: HistoricalActualPosition {
            id: group,
            trace: lineage.trace.id,
            variant: lineage.trace.variant.id,
            parent: node.parent,
            location: location.id,
            packs,
            revision: group_revision,
        },
        location,
        occurred,
        recorded_at,
    };
    source.validate().map_err(|_| corrupt_storage())?;
    Ok(source)
}

/// Every physical event after the explicit source cut; no caller-supplied omissions.
pub(in crate::hub) async fn activity_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    lineage: &lineage::LoadedLineage,
    source: &HistoricalActualSource,
) -> Result<Vec<ActualPhysicalActivity>, SafeError> {
    let rows=sqlx::query(concat!(
        "SELECT e.sequence,substr(e.operation_id,1,37) AS operation_id,substr(e.kind,1,33) AS kind,",
        "substr(e.group_id,1,17) AS group_id,substr(e.to_location_id,1,17) AS to_location_id,",
        "e.occurrence_kind,e.occurrence_start_unix_s,e.occurrence_end_unix_s,o.stock_revision ",
        "FROM hub_stock_events e JOIN hub_stock_operations o ON o.operation_id=e.operation_id ",
        "WHERE e.trace_id=? AND o.hub_id=? AND o.stock_revision>? ORDER BY e.sequence LIMIT ?"
    )).bind(lineage.trace.id.as_bytes().as_slice()).bind(hub.as_bytes().as_slice())
        .bind(integer(source.stock_revision)?).bind(i64::try_from(HUB_STOCK_EVENT_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() > HUB_STOCK_EVENT_CAPACITY {
        return Err(capacity());
    }
    let mut result = Vec::new();
    for row in rows {
        let operation = DurableRequestId::parse(
            row.try_get::<String, _>("operation_id")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind
            != row
                .try_get::<String, _>("kind")
                .map_err(|_| corrupt_storage())?
            || op.revision != revision_field(&row, "stock_revision")?
        {
            return Err(corrupt_storage());
        }
        let revision = op.revision;
        let recorded_at = op.recorded_at;
        let group = record_id(&row, "group_id")?;
        let node = lineage.graph.node(group).ok_or_else(corrupt_storage)?;
        let kind = match op.kind.as_str() {
            "move" => {
                history::movement_on(tx, hub, op).await?;
                if node.created_revision == revision {
                    Activity::GroupBorn
                } else {
                    Activity::LocationChanged
                }
            }
            "split" => {
                portions::original_on(tx, hub, op).await?;
                Activity::GroupBorn
            }
            "count_post" => {
                let original = crate::hub::counts::posting::original_on(tx, hub, op).await?;
                if original
                    .surplus
                    .as_ref()
                    .is_some_and(|s| s.group.id == group && s.population.id == lineage.trace.id)
                {
                    Activity::GroupBorn
                } else {
                    Activity::CountReconciled
                }
            }
            "departure" => {
                departure::read::original_on(tx, hub, op).await?;
                Activity::OtherPhysicalEffect
            }
            "disposition" => {
                crate::hub::custody::disposition::read::original_on(tx, hub, op).await?;
                Activity::OtherPhysicalEffect
            }
            "receipt" | "opening" | "return" => {
                stock::read::population_on(tx, hub, op, lineage.trace.id).await?;
                Activity::GroupBorn
            }
            "count_observe"
            | "acceptance"
            | "hold_place"
            | "hold_release"
            | "work_replan"
            | "work_new"
            | "discrepancy_open"
            | "discrepancy_resolve"
            | "dispatch_prepare"
            | "dispatch_revise"
            | "dispatch_issue"
            | "dispatch_replace"
            | "dispatch_unused" => continue,
            _ => return Err(corrupt_storage()),
        };
        result.push(ActualPhysicalActivity {
            sequence: u32::try_from(
                row.try_get::<i64, _>("sequence")
                    .map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?,
            operation: operation_id(&operation)?,
            stock_revision: revision,
            hub,
            trace: lineage.trace.id,
            group,
            location: stock::read::optional_id(&row, "to_location_id")?,
            kind,
            occurred: stock::read::occurrence(&row)?,
            recorded_at,
        });
    }
    Ok(result)
}

/// Original normal departures for the whole population, including depleted lines.
/// Forward actual-reconciliation storage must add its originals before exposure.
pub(in crate::hub) async fn known_departures_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
) -> Result<Vec<ActualKnownDeparture>, SafeError> {
    let operations:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(o.operation_id,1,37) FROM hub_stock_operations o WHERE o.hub_id=? AND o.kind='departure' ",
        "AND EXISTS(SELECT 1 FROM hub_stock_events e WHERE e.operation_id=o.operation_id AND e.trace_id=?) ORDER BY o.stock_revision LIMIT ?"
    )).bind(hub.as_bytes().as_slice()).bind(trace.as_bytes().as_slice()).bind(i64::try_from(HUB_STOCK_EVENT_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if operations.len() > HUB_STOCK_EVENT_CAPACITY {
        return Err(capacity());
    }
    let mut result = Vec::new();
    for operation in operations {
        let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        let original = departure::read::original_on(tx, hub, op).await?.record;
        let material = &original
            .paper
            .versions
            .last()
            .ok_or_else(corrupt_storage)?
            .material;
        let lines: Vec<_> = material
            .lines
            .iter()
            .filter(|line| line.trace.id == trace)
            .collect();
        let first = lines.first().ok_or_else(corrupt_storage)?;
        let variant = first.variant.clone();
        if lines.iter().any(|line| line.variant != variant) {
            return Err(corrupt_storage());
        }
        let known = ActualKnownDeparture {
            operation: original.operation,
            hub,
            stock_revision: original.stock_revision,
            catalog_revision: original.catalog_revision,
            trace,
            variant,
            recipient: material.recipient.clone(),
            occurred: original.occurred,
            recorded_at: original.recorded_at,
            lines: lines
                .into_iter()
                .map(|line| ActualKnownDepartureLine {
                    group: line.group,
                    location: line.location.clone(),
                    packs: line.packs,
                })
                .collect(),
        };
        known.validate().map_err(|_| corrupt_storage())?;
        result.push(known);
    }
    Ok(result)
}
