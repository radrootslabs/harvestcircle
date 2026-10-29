//! One protected transaction captures every original before any current mutation.
use super::super::{lineage, treatment, write as journal};
use super::*;
use crate::Database;
use harvestcircle_domain::hub::departure::{
    DepartureCommand, DepartureContext, DepartureError, DepartureTreatment, departure_result_digest,
};
use harvestcircle_domain::hub::dispatch::{DispatchSource, IdentificationStatus};
use harvestcircle_domain::hub::movement::WorkProgress;
use std::collections::BTreeMap;

impl Database {
    pub async fn apply_hub_departure(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: DepartureCommand,
        now: UnixTimestamp,
    ) -> Result<HubDepartureReceipt, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move { apply_on(tx, actor, operation, command, now).await })
            })
            .await
            .map_err(map_transaction_error)
    }
}
pub(super) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: DepartureCommand,
    now: UnixTimestamp,
) -> Result<HubDepartureReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let ids = identities(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, ids);
    if let Some(original) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if original.kind != "departure" || original.actor != actor || original.hash != hash {
            return Err(conflict());
        }
        return read::original_on(tx, hub.id(), original).await;
    }
    let state = stock::read::state_on(tx, hub.id()).await?;
    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
    let paper = super::super::dispatch::read::record_on(tx, hub.id(), command.preparation).await?;
    if read::usage_on(tx, hub.id(), paper.id).await?.is_some() {
        return Err(unavailable());
    }
    if command.expected_stock_revision != state.revision
        || command.expected_catalog_revision != catalog.revision()
        || command.expected_preparation_revision != paper.revision
    {
        return Err(conflict());
    }
    let version = paper.versions.last().ok_or_else(unavailable)?;
    if paper.draft.is_some()
        || version.id != command.identification
        || version.status != IdentificationStatus::Issued
        || version.closed.is_some()
    {
        return Err(unavailable());
    }
    let mut traces = BTreeMap::new();
    for line in &version.material.lines {
        if let std::collections::btree_map::Entry::Vacant(slot) = traces.entry(line.trace.id) {
            slot.insert(
                lineage::load_on(tx, hub.id(), line.trace.id, &catalog, state.revision).await?,
            );
        }
    }
    let mut sources = Vec::with_capacity(version.material.lines.len());
    for line in &version.material.lines {
        let loaded = traces.get(&line.trace.id).ok_or_else(corrupt_storage)?;
        let node = loaded.graph.node(line.group).ok_or_else(corrupt_storage)?;
        if node.packs.value() == 0 {
            return Err(unavailable());
        }
        let group = super::super::read::group_on(tx, hub.id(), line.group, &catalog).await?;
        sources.push(DispatchSource {
            group,
            trace: loaded.trace.clone(),
        });
    }
    let mut treatments = Vec::with_capacity(traces.len());
    for loaded in traces.values() {
        treatments.push(DepartureTreatment {
            state: treatment::read::state_on(tx, hub.id(), &loaded.graph).await?,
            lineage: loaded.graph.clone(),
        });
    }
    let plan = command
        .plan(
            &DepartureContext {
                hub: hub.id(),
                stock_revision: state.revision,
                catalog: &catalog,
                paper: &paper,
                sources: &sources,
                treatments: &treatments,
                prior_departure: None,
            },
            ids,
            actor,
            now,
        )
        .map_err(departure_error)?;
    let events = u32::try_from(plan.physical_event_count()).map_err(|_| corrupt_storage())?;
    treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        treatment::capacity::Reservation::Depart(events),
    )
    .await?;
    let original = stock::read::OperationRecord {
        id: operation.clone(),
        kind: "departure".to_owned(),
        actor,
        hash,
        catalog_revision: catalog.revision(),
        revision: plan.stock_revision,
        result: plan.id,
        recorded_at: now,
    };
    journal::insert_operation_on(tx, hub.id(), &original).await?;
    capture_on(tx, &operation, &plan, &hash).await?;
    for effect in &plan.consumed {
        let group = &effect.before;
        let removed = sqlx::query(
            "DELETE FROM hub_stock_positions WHERE group_id=? AND location_id=? AND packs=?",
        )
        .bind(group.id.as_bytes().as_slice())
        .bind(group.location.as_bytes().as_slice())
        .bind(i64::from(group.packs.value()))
        .execute(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
        if removed.rows_affected() != 1 {
            return Err(conflict());
        }
        let revised =
            sqlx::query("UPDATE hub_stock_groups SET revision=? WHERE group_id=? AND revision=?")
                .bind(integer(effect.after_revision)?)
                .bind(group.id.as_bytes().as_slice())
                .bind(integer(group.revision)?)
                .execute(&mut *tx)
                .await
                .map_err(|_| storage_unavailable())?;
        if revised.rows_affected() != 1 {
            return Err(conflict());
        }
        let retired=sqlx::query("UPDATE hub_group_work SET progress='no_longer_required' WHERE group_id=? AND progress='pending'")
            .bind(group.id.as_bytes().as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if retired.rows_affected()
            != group
                .work
                .iter()
                .filter(|allocation| allocation.progress == WorkProgress::Pending)
                .count() as u64
        {
            return Err(conflict());
        }
    }
    for change in &plan.work_changes {
        journal::update_work_on(tx, &change.before, &change.after).await?;
        journal::work_change_on(tx, &operation, Some(&change.before), &change.after).await?;
    }
    for (ordinal, line) in plan
        .paper
        .versions
        .last()
        .ok_or_else(corrupt_storage)?
        .material
        .lines
        .iter()
        .enumerate()
    {
        let prior_events = state
            .event_count
            .checked_add(u32::try_from(ordinal).map_err(|_| corrupt_storage())?)
            .ok_or_else(capacity)?;
        journal::event_on(
            tx,
            &operation,
            journal::EventEffect {
                kind: "departure",
                prior_events,
                trace: line.trace.id,
                group: line.group,
                packs: line.packs,
                from: Some(&line.location),
                to: None,
                occurred: plan.occurred,
            },
        )
        .await?;
    }
    journal::advance_many_on(tx, hub.id(), &state, plan.stock_revision, events).await?;
    // Actual readers must accept the conserved remainder and complete work scope.
    let _ = stock::read::state_on(tx, hub.id()).await?;
    for trace in traces.keys() {
        let _ = lineage::load_on(tx, hub.id(), *trace, &catalog, plan.stock_revision).await?;
    }
    for change in &plan.work_changes {
        if super::super::read::work_on(tx, hub.id(), change.after.id).await? != change.after {
            return Err(corrupt_storage());
        }
    }
    let restored = read::original_on(tx, hub.id(), original).await?;
    if restored.record != plan {
        return Err(corrupt_storage());
    }
    Ok(restored)
}
async fn capture_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    record: &DepartureRecord,
    input: &[u8; 32],
) -> Result<(), SafeError> {
    let version = record.paper.versions.last().ok_or_else(corrupt_storage)?;
    let digest = departure_result_digest(input, record).map_err(|_| corrupt_storage())?;
    let (kind, start, end) = occurrence_fields(record.occurred);
    sqlx::query(concat!(
        "INSERT INTO hub_departures (operation_id,departure_id,preparation_id,preparation_revision,identification_id,line_count,work_change_count,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,observation,result_hash) ",
        "VALUES (?,?,?,?,?,?,?,?,?,?,?,?)"
    )).bind(operation.as_str()).bind(record.id.as_bytes().as_slice()).bind(record.paper.id.as_bytes().as_slice()).bind(integer(record.paper.revision)?)
        .bind(version.id.as_bytes().as_slice()).bind(i64::try_from(record.consumed.len()).map_err(|_|corrupt_storage())?).bind(i64::try_from(record.work_changes.len()).map_err(|_|corrupt_storage())?)
        .bind(kind).bind(start).bind(end).bind(record.observation.as_str()).bind(digest.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    for (ordinal, effect) in record.consumed.iter().enumerate() {
        let group = &effect.before;
        let line = version
            .material
            .lines
            .get(ordinal)
            .ok_or_else(corrupt_storage)?;
        sqlx::query(concat!(
            "INSERT INTO hub_departure_lines (operation_id,ordinal,group_id,trace_id,before_group_revision,after_group_revision,packs,location_id,location_revision,before_causal_floor_unix_s,after_causal_floor_unix_s,scope_count) ",
            "VALUES (?,?,?,?,?,?,?,?,?,?,?,?)"
        )).bind(operation.as_str()).bind(ordinal as i64).bind(group.id.as_bytes().as_slice()).bind(group.trace.as_bytes().as_slice()).bind(integer(group.revision)?).bind(integer(effect.after_revision)?)
            .bind(i64::from(group.packs.value())).bind(group.location.as_bytes().as_slice()).bind(integer(line.location.revision)?)
            .bind(group.causal_floor.map(UnixTimestamp::as_seconds)).bind(effect.causal_floor.map(UnixTimestamp::as_seconds)).bind(group.work.len() as i64)
            .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
        for allocation in &group.work {
            let before = &allocation.requirement;
            sqlx::query(concat!(
                "INSERT INTO hub_departure_work_scope (operation_id,line_ordinal,work_id,before_progress,before_work_revision,before_completed,before_no_longer_required,before_remaining) ",
                "VALUES (?,?,?,?,?,?,?,?)"
            )).bind(operation.as_str()).bind(ordinal as i64).bind(before.id.as_bytes().as_slice()).bind(journal::progress(allocation.progress)).bind(integer(before.revision)?)
                .bind(i64::from(before.quantity.completed().value())).bind(i64::from(before.quantity.no_longer_required().value())).bind(i64::from(before.quantity.remaining().value()))
                .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
        }
    }
    Ok(())
}
fn unavailable() -> SafeError {
    SafeError::new(
        SafeErrorCode::HubConflict,
        SafeMessage::new(
            "The selected identification or stock is no longer available for departure. Review current records.",
        ),
    )
}
fn departure_error(error: DepartureError) -> SafeError {
    use harvestcircle_domain::hub::{dispatch::DispatchError, treatment::TreatmentError};
    match error {
        DepartureError::AlreadyUsed | DepartureError::IdentificationUnavailable => unavailable(),
        DepartureError::MaterialChanged
        | DepartureError::StaleRevision
        | DepartureError::Dispatch(DispatchError::StaleRevision) => SafeError::new(
            SafeErrorCode::HubConflict,
            SafeMessage::new(
                "Departure material changed. Review current stock and replace identification where required.",
            ),
        ),
        DepartureError::Treatment(TreatmentError::ActionBlocked) => SafeError::new(
            SafeErrorCode::HubConflict,
            SafeMessage::new(
                "Current sample acceptance or an active hold blocks normal departure. No stock was removed.",
            ),
        ),
        DepartureError::Capacity
        | DepartureError::RevisionExhausted
        | DepartureError::Treatment(TreatmentError::Capacity) => capacity(),
        DepartureError::Movement(harvestcircle_domain::hub::movement::MovementError::Stock(
            harvestcircle_domain::hub::stock::StockError::InvalidOccurrence,
        )) => SafeError::new(
            SafeErrorCode::HubInvalidInput,
            SafeMessage::new(
                "Review actual occurrence time and the selected stock's causal history.",
            ),
        ),
        _ => corrupt_storage(),
    }
}
