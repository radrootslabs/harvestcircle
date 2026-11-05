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
    graph: &harvestcircle_domain::hub::portions::PhysicalLineage,
    trace: &StockPopulation,
    operation: &DurableRequestId,
    group: HubRecordId,
) -> Result<HistoricalActualSource, SafeError> {
    if graph.hub() != hub || graph.trace() != trace.id || graph.variant() != trace.variant.id {
        return Err(corrupt_storage());
    }
    let node = graph.node(group).ok_or_else(invalid_input)?;
    let original = source_original_on(tx, hub, trace, operation, group).await?;
    if node.parent != original.group.parent
        || node.created_revision.value() > original.stock_revision.value()
        || original.stock_revision.value() > graph.revision().value()
    {
        return Err(invalid_input());
    }
    Ok(original)
}

/// Immutable source projection without consulting current balances or recursively
/// reconstructing actual-review history. Full callers still verify their graph cut.
pub(in crate::hub) async fn source_original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: &StockPopulation,
    operation: &DurableRequestId,
    group: HubRecordId,
) -> Result<HistoricalActualSource, SafeError> {
    let row=sqlx::query(concat!(
        "SELECT substr(g.trace_id,1,17) AS trace_id,substr(i.parent_group_id,1,17) AS parent_id,",
        "substr(i.created_operation_id,1,37) AS birth_operation,substr(CAST(i.physical_label AS BLOB),1,121) AS label,",
        "substr(CAST(i.observation AS BLOB),1,513) AS observation FROM hub_stock_groups g JOIN hub_group_identification i ON i.group_id=g.group_id WHERE g.group_id=? LIMIT 1"
    )).bind(group.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(invalid_input)?;
    if record_id(&row, "trace_id")? != trace.id {
        return Err(invalid_input());
    }
    let parent = stock::read::optional_id(&row, "parent_id")?;
    let birth_operation = DurableRequestId::parse(
        row.try_get::<String, _>("birth_operation")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let born = stock::read::operation_on(tx, hub, &birth_operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    let born_revision = born.revision;
    let (_, identification, _, _) =
        Box::pin(lineage::creation_on(tx, hub, trace, group, parent, born)).await?;
    let label: Option<Vec<u8>> = row.try_get("label").map_err(|_| corrupt_storage())?;
    let observation: Option<Vec<u8>> = row.try_get("observation").map_err(|_| corrupt_storage())?;
    let stored = match (label, observation) {
        (None, None) => None,
        (Some(label), Some(observation)) => Some(PhysicalIdentification {
            label: HubLabel::new(String::from_utf8(label).map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?,
            observation: harvestcircle_domain::hub::reference::SourceProvenance::new(
                String::from_utf8(observation).map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?,
        }),
        _ => return Err(corrupt_storage()),
    };
    if identification != stored {
        return Err(corrupt_storage());
    }
    let op = stock::read::operation_on(tx, hub, operation)
        .await?
        .ok_or_else(invalid_input)?;
    let revision = op.revision;
    let catalog_revision = op.catalog_revision;
    let recorded_at = op.recorded_at;
    let source_actor = op.actor;
    if revision.value() < born_revision.value() {
        return Err(invalid_input());
    }
    let (location, packs, group_revision, occurred) = match op.kind.as_str() {
        "receipt" | "opening" | "count_post" | "return"
            if parent.is_none() && born_revision == revision =>
        {
            let population = Box::pin(stock::read::population_on(tx, hub, op, trace.id)).await?;
            if population != *trace || birth_operation.as_str() != operation.as_str() {
                return Err(invalid_input());
            }
            (
                population.initial_location,
                population.original_packs,
                HubRevision::INITIAL,
                population.occurred,
            )
        }
        "correction_found" if birth_operation.as_str() == operation.as_str() => {
            let original = crate::hub::corrections::birth::birth_on(tx, hub, &op, group).await?;
            if original.group.trace != trace.id
                || original.group.variant != trace.variant.id
                || original.group.parent != parent
                || original.birth.stock_revision != born_revision
            {
                return Err(corrupt_storage());
            }
            (
                original.location,
                original.group.packs,
                original.group.revision,
                original.birth.occurred,
            )
        }
        "move" => {
            let original = history::movement_on(tx, hub, op).await?;
            let HubHandlingResult::Moved(moved) = original.result else {
                return Err(corrupt_storage());
            };
            if original.trace != trace.id || moved.moved_group != group {
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
            if original.trace != trace.id || split.portion_group != group {
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
        actor: source_actor,
        hub,
        stock_revision: revision,
        catalog_revision,
        group: HistoricalActualPosition {
            id: group,
            trace: trace.id,
            variant: trace.variant.id,
            parent,
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
    graph: &harvestcircle_domain::hub::portions::PhysicalLineage,
    source: &HistoricalActualSource,
) -> Result<Vec<ActualPhysicalActivity>, SafeError> {
    if graph.hub() != hub
        || source.hub != hub
        || source.group.trace != graph.trace()
        || source.stock_revision.value() > graph.revision().value()
    {
        return Err(corrupt_storage());
    }
    let rows=sqlx::query(concat!(
        "SELECT e.sequence,substr(e.operation_id,1,37) AS operation_id,substr(e.kind,1,33) AS kind,",
        "substr(e.group_id,1,17) AS group_id,substr(e.to_location_id,1,17) AS to_location_id,",
        "e.occurrence_kind,e.occurrence_start_unix_s,e.occurrence_end_unix_s,o.stock_revision ",
        "FROM hub_stock_events e JOIN hub_stock_operations o ON o.operation_id=e.operation_id ",
        "WHERE e.trace_id=? AND o.hub_id=? AND o.stock_revision>? AND o.stock_revision<=? ORDER BY e.sequence LIMIT ?"
    )).bind(graph.trace().as_bytes().as_slice()).bind(hub.as_bytes().as_slice())
        .bind(integer(source.stock_revision)?).bind(integer(graph.revision())?).bind(i64::try_from(HUB_STOCK_EVENT_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
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
        let activity_actor = op.actor;
        let group = record_id(&row, "group_id")?;
        let node = graph.node(group).ok_or_else(corrupt_storage)?;
        let kind = match op.kind.as_str() {
            "correction_found" => {
                let birth = crate::hub::corrections::birth::birth_on(tx, hub, &op, group).await?;
                if node.created_revision != revision
                    || birth.group.trace != graph.trace()
                    || birth.group.parent != node.parent
                    || node.recovery != Some(birth.recovery)
                {
                    return Err(corrupt_storage());
                }
                Activity::GroupBorn
            }
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
                    .is_some_and(|s| s.group.id == group && s.population.id == graph.trace())
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
            "actual_review" => {
                crate::hub::actuals::projection::known_on(tx, hub, &op).await?;
                Activity::OtherPhysicalEffect
            }
            "correction_additional" => {
                let debit = Box::pin(
                    crate::hub::corrections::additional_projection::physical_debit_on(tx, hub, &op),
                )
                .await?;
                if debit.before.id != group
                    || debit.before.trace != graph.trace()
                    || debit.before.variant != graph.variant()
                    || node.created_revision.value() >= revision.value()
                {
                    return Err(corrupt_storage());
                }
                Activity::OtherPhysicalEffect
            }
            "receipt" | "opening" | "return" => {
                stock::read::population_on(tx, hub, op, graph.trace()).await?;
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
            actor: activity_actor,
            stock_revision: revision,
            hub,
            trace: graph.trace(),
            group,
            location: stock::read::optional_id(&row, "to_location_id")?,
            kind,
            occurred: stock::read::occurrence(&row)?,
            recorded_at,
        });
    }
    Ok(result)
}

/// Original normal and reconciled departures for the whole population, including
/// depleted lines. Journal-only report/link rows contribute no physical event.
pub(in crate::hub) async fn known_departures_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
    cut: HubRevision,
) -> Result<Vec<ActualKnownDeparture>, SafeError> {
    let operations:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(o.operation_id,1,37) FROM hub_stock_operations o WHERE o.hub_id=? AND o.kind IN ('departure','actual_review') AND o.stock_revision<=? ",
        "AND EXISTS(SELECT 1 FROM hub_stock_events e WHERE e.operation_id=o.operation_id AND e.trace_id=?) ORDER BY o.stock_revision LIMIT ?"
    )).bind(hub.as_bytes().as_slice()).bind(integer(cut)?).bind(trace.as_bytes().as_slice()).bind(i64::try_from(HUB_STOCK_EVENT_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if operations.len() > HUB_STOCK_EVENT_CAPACITY {
        return Err(capacity());
    }
    let mut result = Vec::new();
    for operation in operations {
        let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        if op.kind == "actual_review" {
            let known = crate::hub::actuals::projection::known_on(tx, hub, &op).await?;
            if known.trace != trace {
                return Err(corrupt_storage());
            }
            result.push(known);
            continue;
        }
        let known = known_departure_on(tx, hub, trace, op).await?;
        result.push(known);
    }
    Ok(result)
}

/// Exact named normal event, with every line of the selected population.
pub(in crate::hub) async fn known_departure_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
    op: stock::read::OperationRecord,
) -> Result<ActualKnownDeparture, SafeError> {
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
        actor: original.actor,
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
    Ok(known)
}

/// Canonical ledger outflow at one cut, grouped by exact physical identity.
/// Birth conservation includes retained zero-pack branches. Surplus belongs to a
/// different population and no-variance/draft/report/link records debit nothing.
pub(in crate::hub) async fn physical_outflows_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    graph: &harvestcircle_domain::hub::portions::PhysicalLineage,
) -> Result<std::collections::BTreeMap<HubRecordId, WholePacks>, SafeError> {
    let operations:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(o.operation_id,1,37) FROM hub_stock_operations o WHERE o.hub_id=? AND o.stock_revision<=? ",
        "AND o.kind IN ('departure','count_post','disposition','actual_review') AND EXISTS(SELECT 1 FROM hub_stock_events e WHERE e.operation_id=o.operation_id AND e.trace_id=?) ORDER BY o.stock_revision LIMIT ?"
    )).bind(hub.as_bytes().as_slice()).bind(integer(graph.revision())?).bind(graph.trace().as_bytes().as_slice())
        .bind(i64::try_from(HUB_STOCK_EVENT_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if operations.len() > HUB_STOCK_EVENT_CAPACITY {
        return Err(capacity());
    }
    let mut totals = std::collections::BTreeMap::new();
    for operation in operations {
        let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        let revision = op.revision;
        let corrected = matches!(op.kind.as_str(), "departure" | "actual_review");
        let outflows: Vec<(HubRecordId, WholePacks)> = match op.kind.as_str() {
            "actual_review" => {
                let known = crate::hub::actuals::projection::known_on(tx, hub, &op).await?;
                if known.trace != graph.trace() {
                    return Err(corrupt_storage());
                }
                known
                    .lines
                    .into_iter()
                    .map(|line| (line.group, line.packs))
                    .collect()
            }
            "departure" => departure::read::original_on(tx, hub, op)
                .await?
                .record
                .consumed
                .into_iter()
                .filter(|line| line.before.trace == graph.trace())
                .map(|line| (line.before.id, line.before.packs))
                .collect(),
            "count_post" => {
                let original = crate::hub::counts::posting::original_on(tx, hub, op)
                    .await?
                    .original;
                match original.effect {
                    harvestcircle_domain::hub::counts::CountEffect::Shortage { packs, .. }
                        if original.before.trace == graph.trace() =>
                    {
                        vec![(original.before.id, packs)]
                    }
                    _ => vec![],
                }
            }
            "disposition" => {
                let original = crate::hub::custody::disposition::read::original_on(tx, hub, op)
                    .await?
                    .original;
                if original.consumed.before.trace != graph.trace() {
                    return Err(corrupt_storage());
                }
                vec![(original.consumed.before.id, original.consumed.before.packs)]
            }
            _ => return Err(corrupt_storage()),
        };
        for (group, packs) in outflows {
            let node = graph.node(group).ok_or_else(corrupt_storage)?;
            if node.created_revision.value() >= revision.value() || packs.value() == 0 {
                return Err(corrupt_storage());
            }
            let packs = if corrected {
                Box::pin(
                    crate::hub::corrections::physical_balance::original_physical_net_on(
                        tx,
                        hub,
                        &operation,
                        group,
                        packs,
                        graph.revision(),
                    ),
                )
                .await?
            } else {
                packs
            };
            let prior = totals
                .entry(group)
                .or_insert(WholePacks::from_value(0).expect("zero"));
            *prior = prior.checked_add(packs).map_err(|_| corrupt_storage())?;
        }
    }
    for (group, packs) in
        Box::pin(crate::hub::corrections::physical_balance::additional_outflows_on(tx, hub, graph))
            .await?
    {
        let total = totals
            .entry(group)
            .or_insert(WholePacks::from_value(0).expect("zero"));
        *total = total.checked_add(packs).map_err(|_| corrupt_storage())?;
    }
    Ok(totals)
}
