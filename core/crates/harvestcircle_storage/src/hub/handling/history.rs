//! Original results use captured events and work versions, never current stock.

use super::super::{reference, stock};
use super::*;
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::movement::{
    HUB_GROUP_REQUIREMENTS, HUB_REQUIREMENT_GROUPS, MovementCommand, NewWorkCommand, ReplanCommand,
};
use harvestcircle_domain::hub::reference::{ReferenceKind, SourceProvenance};
use harvestcircle_domain::hub::stock::QuantityWork;
use sqlx::Row;
use stock::read::{
    OperationRecord, optional_id, optional_record_on, record_id, revision_field, text, whole_packs,
};

impl Database {
    pub async fn hub_handling_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubHandlingReceipt>, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    let hub = reference::authorize_on(transaction, actor).await?;
                    reference::validate_basis_on(transaction, hub.id()).await?;
                    match stock::read::operation_on(transaction, hub.id(), &operation).await? {
                        Some(operation) => Ok(Some(match operation.kind.as_str() {
                            "move" => movement_on(transaction, hub.id(), operation).await?,
                            "split" => {
                                portions::original_on(transaction, hub.id(), operation).await?
                            }
                            "acceptance" | "hold_place" | "hold_release" => {
                                treatment::original_on(transaction, hub.id(), operation).await?
                            }
                            "work_replan" | "work_new" => {
                                work_on(transaction, hub.id(), operation).await?
                            }
                            "discrepancy_open" | "discrepancy_resolve" => {
                                discrepancy::original_on(transaction, hub.id(), operation).await?
                            }
                            _ => return Err(conflict()),
                        })),
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn movement_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: OperationRecord,
) -> Result<HubHandlingReceipt, SafeError> {
    if HubHandlingKind::parse(&operation.kind)? != HubHandlingKind::Move {
        return Err(conflict());
    }
    let row=sqlx::query(concat!(
        "SELECT substr(d.selected_group_id,1,17) AS selected_group_id,d.expected_group_revision,d.source_before_packs,substr(d.moved_group_id,1,17) AS moved_group_id,d.work_change_count,",
        "substr(CAST(d.observation AS BLOB),1,513) AS observation,e.sequence,substr(e.kind,1,33) AS event_kind,substr(e.trace_id,1,17) AS trace_id,substr(e.group_id,1,17) AS event_group,e.packs,",
        "substr(e.from_location_id,1,17) AS from_location_id,e.from_location_revision,substr(e.to_location_id,1,17) AS to_location_id,e.to_location_revision,",
        "substr(e.occurrence_kind,1,9) AS occurrence_kind,e.occurrence_start_unix_s,e.occurrence_end_unix_s ",
        "FROM hub_movement_details d JOIN hub_stock_events e ON e.operation_id=d.operation_id WHERE d.operation_id=? LIMIT 1"
    )).bind(operation.id.as_str()).fetch_optional(&mut *transaction).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    single_event_on(transaction, &operation.id).await?;
    let ids = handling_ids(&operation.id).map_err(|_| corrupt_storage())?;
    let selected = record_id(&row, "selected_group_id")?;
    let moved = record_id(&row, "moved_group_id")?;
    let trace = record_id(&row, "trace_id")?;
    let before = whole_packs(
        row.try_get("source_before_packs")
            .map_err(|_| corrupt_storage())?,
    )?;
    let packs = whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?;
    let sequence: i64 = row.try_get("sequence").map_err(|_| corrupt_storage())?;
    if packs.value() == 0
        || before.value() < packs.value()
        || moved != operation.result
        || moved != record_id(&row, "event_group")?
        || row
            .try_get::<String, _>("event_kind")
            .map_err(|_| corrupt_storage())?
            != "move"
        || !(1..=harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY as i64)
            .contains(&sequence)
    {
        return Err(corrupt_storage());
    }
    let selected_trace: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT substr(trace_id,1,17) FROM hub_stock_groups WHERE group_id=? LIMIT 1",
    )
    .bind(selected.as_bytes().as_slice())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| storage_unavailable())?;
    if selected_trace.as_deref() != Some(trace.as_bytes().as_slice()) {
        return Err(corrupt_storage());
    }
    let separated = if before != packs {
        if selected == moved || moved != ids.group {
            return Err(corrupt_storage());
        }
        let identity=sqlx::query("SELECT substr(parent_group_id,1,17) AS parent_group_id,substr(CAST(physical_label AS BLOB),1,121) AS physical_label,substr(CAST(observation AS BLOB),1,513) AS observation,substr(created_operation_id,1,37) AS created_operation_id FROM hub_group_identification WHERE group_id=? LIMIT 1")
            .bind(moved.as_bytes().as_slice()).fetch_optional(&mut *transaction).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
        if optional_id(&identity, "parent_group_id")? != Some(selected)
            || identity
                .try_get::<String, _>("created_operation_id")
                .map_err(|_| corrupt_storage())?
                != operation.id.as_str()
        {
            return Err(corrupt_storage());
        }
        Some(PhysicalIdentification {
            label: HubLabel::new(text(&identity, "physical_label")?)
                .map_err(|_| corrupt_storage())?,
            observation: SourceProvenance::new(text(&identity, "observation")?)
                .map_err(|_| corrupt_storage())?,
        })
    } else {
        if selected != moved {
            return Err(corrupt_storage());
        }
        None
    };
    let from = optional_record_on(
        transaction,
        hub,
        &row,
        "from_location_id",
        "from_location_revision",
    )
    .await?
    .ok_or_else(corrupt_storage)?;
    let to = optional_record_on(
        transaction,
        hub,
        &row,
        "to_location_id",
        "to_location_revision",
    )
    .await?
    .ok_or_else(corrupt_storage)?;
    if from.id == to.id
        || from.value.kind() != ReferenceKind::Location
        || to.value.kind() != ReferenceKind::Location
    {
        return Err(corrupt_storage());
    }
    let occurred = stock::read::occurrence(&row)?;
    occurred
        .validate(operation.recorded_at)
        .map_err(|_| corrupt_storage())?;
    let expected_stock = HubRevision::from_value(
        operation
            .revision
            .value()
            .checked_sub(1)
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let command = MovementCommand {
        expected_stock_revision: expected_stock,
        expected_catalog_revision: operation.catalog_revision,
        group: selected,
        expected_group_revision: revision_field(&row, "expected_group_revision")?,
        to: to.id,
        packs,
        separated_portion: separated.clone(),
        occurred,
        observation: SourceProvenance::new(text(&row, "observation")?)
            .map_err(|_| corrupt_storage())?,
    };
    if command.fingerprint(hub, operation.actor, ids.group) != operation.hash {
        return Err(corrupt_storage());
    }
    let changes = work_changes_on(transaction, hub, &operation.id).await?;
    let count: i64 = row
        .try_get("work_change_count")
        .map_err(|_| corrupt_storage())?;
    if count < 0 || count as usize > HUB_GROUP_REQUIREMENTS || count as usize != changes.len() {
        return Err(corrupt_storage());
    }
    for change in &changes {
        let prior = change.before.as_ref().ok_or_else(corrupt_storage)?;
        if prior.trace != trace
            || prior.target.as_ref().map(|record| record.id) != Some(to.id)
            || prior
                .quantity
                .completed()
                .checked_add(packs)
                .map_err(|_| corrupt_storage())?
                != change.after.quantity.completed()
            || prior
                .quantity
                .remaining()
                .checked_sub(packs)
                .map_err(|_| corrupt_storage())?
                != change.after.quantity.remaining()
            || prior.quantity.no_longer_required() != change.after.quantity.no_longer_required()
        {
            return Err(corrupt_storage());
        }
    }
    Ok(HubHandlingReceipt {
        operation: operation.id,
        kind: HubHandlingKind::Move,
        revision: operation.revision,
        catalog_revision: operation.catalog_revision,
        trace,
        actor: operation.actor,
        recorded_at: operation.recorded_at,
        result: HubHandlingResult::Moved(HubMovementResult {
            selected_group: selected,
            expected_group_revision: command.expected_group_revision,
            source_before_packs: before,
            from,
            to,
            packs,
            moved_group: moved,
            remaining_packs: before.checked_sub(packs).map_err(|_| corrupt_storage())?,
            separated_portion: separated,
            occurred,
            observation: command.observation,
            work_changes: changes,
        }),
    })
}

pub(super) async fn work_changes_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &DurableRequestId,
) -> Result<Vec<HubWorkChange>, SafeError> {
    let selected=sqlx::query("SELECT substr(work_id,1,17) AS work_id,before_completed,before_no_longer_required,before_remaining,before_revision,after_completed,after_no_longer_required,after_remaining,after_revision FROM hub_work_changes WHERE operation_id=? ORDER BY work_id LIMIT 65")
        .bind(operation.as_str()).fetch_all(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if selected.len() > HUB_GROUP_REQUIREMENTS {
        return Err(corrupt_storage());
    }
    let mut changes = Vec::with_capacity(selected.len());
    for row in selected {
        let definition = read::definition_on(transaction, hub, record_id(&row, "work_id")?).await?;
        let after = definition.at(
            QuantityWork::restore(
                definition.required,
                whole_packs(
                    row.try_get("after_completed")
                        .map_err(|_| corrupt_storage())?,
                )?,
                whole_packs(
                    row.try_get("after_no_longer_required")
                        .map_err(|_| corrupt_storage())?,
                )?,
                whole_packs(
                    row.try_get("after_remaining")
                        .map_err(|_| corrupt_storage())?,
                )?,
            )
            .map_err(|_| corrupt_storage())?,
            revision_field(&row, "after_revision")?,
        )?;
        let before_revision: Option<i64> = row
            .try_get("before_revision")
            .map_err(|_| corrupt_storage())?;
        let completed: Option<i64> = row
            .try_get("before_completed")
            .map_err(|_| corrupt_storage())?;
        let retired: Option<i64> = row
            .try_get("before_no_longer_required")
            .map_err(|_| corrupt_storage())?;
        let remaining: Option<i64> = row
            .try_get("before_remaining")
            .map_err(|_| corrupt_storage())?;
        let before = match (before_revision, completed, retired, remaining) {
            (None, None, None, None)
                if after.revision == HubRevision::INITIAL
                    && definition.created_operation == *operation
                    && after.quantity.completed().value() == 0
                    && after.quantity.no_longer_required().value() == 0 =>
            {
                None
            }
            (Some(revision), Some(completed), Some(retired), Some(remaining)) => {
                let prior = definition.at(
                    QuantityWork::restore(
                        definition.required,
                        whole_packs(completed)?,
                        whole_packs(retired)?,
                        whole_packs(remaining)?,
                    )
                    .map_err(|_| corrupt_storage())?,
                    stock::read::revision(revision)?,
                )?;
                if revision.checked_add(1) != Some(integer(after.revision)?)
                    || after.quantity.completed().value() < prior.quantity.completed().value()
                    || after.quantity.no_longer_required().value()
                        < prior.quantity.no_longer_required().value()
                    || after.quantity.remaining().value() > prior.quantity.remaining().value()
                {
                    return Err(corrupt_storage());
                }
                Some(prior)
            }
            _ => return Err(corrupt_storage()),
        };
        changes.push(HubWorkChange { before, after });
    }
    Ok(changes)
}

pub(super) async fn work_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: OperationRecord,
) -> Result<HubHandlingReceipt, SafeError> {
    let kind = HubHandlingKind::parse(&operation.kind)?;
    if !matches!(kind, HubHandlingKind::Replan | HubHandlingKind::NewWork) {
        return Err(conflict());
    }
    single_event_on(transaction, &operation.id).await?;
    let ids = handling_ids(&operation.id).map_err(|_| corrupt_storage())?;
    let row=sqlx::query("SELECT substr(previous_work_id,1,17) AS previous_work_id,expected_work_revision,substr(replacement_work_id,1,17) AS replacement_work_id,substr(CAST(observation AS BLOB),1,513) AS observation FROM hub_work_actions WHERE operation_id=? LIMIT 1")
        .bind(operation.id.as_str()).fetch_optional(&mut *transaction).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let replacement_id = record_id(&row, "replacement_work_id")?;
    if replacement_id != operation.result || replacement_id != ids.requirement {
        return Err(corrupt_storage());
    }
    let changes = work_changes_on(transaction, hub, &operation.id).await?;
    let created = changes
        .iter()
        .find(|change| change.after.id == replacement_id)
        .ok_or_else(corrupt_storage)?;
    if created.before.is_some()
        || created.after.revision != HubRevision::INITIAL
        || created.after.quantity != QuantityWork::pending(created.after.quantity.required())
    {
        return Err(corrupt_storage());
    }
    let replacement = created.after.clone();
    let target = replacement.target.as_ref().ok_or_else(corrupt_storage)?;
    let selected=sqlx::query("SELECT substr(a.group_id,1,17) AS group_id,a.before_revision,a.scoped_packs,substr(a.location_id,1,17) AS location_id,a.location_revision,substr(g.trace_id,1,17) AS trace_id FROM hub_action_groups a JOIN hub_stock_groups g ON g.group_id=a.group_id WHERE a.operation_id=? ORDER BY a.group_id LIMIT 257")
        .bind(operation.id.as_str()).fetch_all(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if selected.is_empty() || selected.len() > HUB_REQUIREMENT_GROUPS {
        return Err(corrupt_storage());
    }
    let mut groups = Vec::with_capacity(selected.len());
    let mut total = WholePacks::from_value(0).map_err(|_| corrupt_storage())?;
    for row in selected {
        if record_id(&row, "trace_id")? != replacement.trace {
            return Err(corrupt_storage());
        }
        let location =
            optional_record_on(transaction, hub, &row, "location_id", "location_revision")
                .await?
                .ok_or_else(corrupt_storage)?;
        if location.value.kind() != ReferenceKind::Location || location.id == target.id {
            return Err(corrupt_storage());
        }
        let packs = whole_packs(row.try_get("scoped_packs").map_err(|_| corrupt_storage())?)?;
        if packs.value() == 0 {
            return Err(corrupt_storage());
        }
        total = total.checked_add(packs).map_err(|_| corrupt_storage())?;
        groups.push(HubActionGroup {
            group: record_id(&row, "group_id")?,
            before_revision: revision_field(&row, "before_revision")?,
            packs,
            location,
        });
    }
    if total != replacement.quantity.required() {
        return Err(corrupt_storage());
    }
    let observation =
        SourceProvenance::new(text(&row, "observation")?).map_err(|_| corrupt_storage())?;
    let stock = HubRevision::from_value(
        operation
            .revision
            .value()
            .checked_sub(1)
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let previous_id = optional_id(&row, "previous_work_id")?;
    let expected_revision: Option<i64> = row
        .try_get("expected_work_revision")
        .map_err(|_| corrupt_storage())?;
    let previous = match kind {
        HubHandlingKind::Replan => {
            if changes.len() != 2 {
                return Err(corrupt_storage());
            }
            let id = previous_id.ok_or_else(corrupt_storage)?;
            let expected = stock::read::revision(expected_revision.ok_or_else(corrupt_storage)?)?;
            let change = changes
                .iter()
                .find(|change| change.after.id == id)
                .ok_or_else(corrupt_storage)?
                .clone();
            let before = change.before.as_ref().ok_or_else(corrupt_storage)?;
            if before.trace != replacement.trace
                || before.variant != replacement.variant
                || before.revision != expected
                || before.quantity.remaining() != total
                || change.after.quantity.remaining().value() != 0
                || change.after.quantity.completed() != before.quantity.completed()
                || change.after.quantity.no_longer_required()
                    != before
                        .quantity
                        .no_longer_required()
                        .checked_add(total)
                        .map_err(|_| corrupt_storage())?
            {
                return Err(corrupt_storage());
            }
            let command = ReplanCommand {
                expected_stock_revision: stock,
                expected_catalog_revision: operation.catalog_revision,
                requirement: id,
                expected_work_revision: expected,
                target: target.id,
                observation: observation.clone(),
            };
            if command.fingerprint(hub, operation.actor, ids.requirement) != operation.hash {
                return Err(corrupt_storage());
            }
            Some(change)
        }
        HubHandlingKind::NewWork => {
            if changes.len() != 1 || previous_id.is_some() || expected_revision.is_some() {
                return Err(corrupt_storage());
            }
            let command = NewWorkCommand {
                expected_stock_revision: stock,
                expected_catalog_revision: operation.catalog_revision,
                groups: groups.iter().map(|group| group.group).collect(),
                target: target.id,
                observation: observation.clone(),
            };
            command.validate_shape().map_err(|_| corrupt_storage())?;
            if command.fingerprint(hub, operation.actor, ids.requirement) != operation.hash {
                return Err(corrupt_storage());
            }
            None
        }
        _ => return Err(corrupt_storage()),
    };
    validate_nonphysical_event_on(transaction, &operation, replacement.trace).await?;
    Ok(HubHandlingReceipt {
        operation: operation.id,
        kind,
        revision: operation.revision,
        catalog_revision: operation.catalog_revision,
        trace: replacement.trace,
        actor: operation.actor,
        recorded_at: operation.recorded_at,
        result: HubHandlingResult::WorkChanged(HubWorkActionResult {
            replacement,
            previous,
            groups,
            observation,
        }),
    })
}

pub(super) async fn validate_nonphysical_event_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    operation: &OperationRecord,
    trace: HubRecordId,
) -> Result<(), SafeError> {
    let row=sqlx::query("SELECT e.sequence,substr(e.kind,1,33) AS kind,substr(e.trace_id,1,17) AS trace_id,substr(e.group_id,1,17) AS group_id,e.packs,substr(e.from_location_id,1,17) AS from_location_id,e.from_location_revision,substr(e.to_location_id,1,17) AS to_location_id,e.to_location_revision,substr(e.occurrence_kind,1,9) AS occurrence_kind,e.occurrence_start_unix_s,e.occurrence_end_unix_s,substr(r.root_group_id,1,17) AS root_group_id FROM hub_stock_events e JOIN hub_receipts r ON r.trace_id=e.trace_id WHERE e.operation_id=? LIMIT 1")
        .bind(operation.id.as_str()).fetch_optional(&mut *transaction).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    single_event_on(transaction, &operation.id).await?;
    if !(1..=harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY as i64).contains(
        &row.try_get::<i64, _>("sequence")
            .map_err(|_| corrupt_storage())?,
    ) || row
        .try_get::<String, _>("kind")
        .map_err(|_| corrupt_storage())?
        != operation.kind
        || record_id(&row, "trace_id")? != trace
        || record_id(&row, "group_id")? != record_id(&row, "root_group_id")?
        || row
            .try_get::<i64, _>("packs")
            .map_err(|_| corrupt_storage())?
            != 0
        || optional_id(&row, "from_location_id")?.is_some()
        || row
            .try_get::<Option<i64>, _>("from_location_revision")
            .map_err(|_| corrupt_storage())?
            .is_some()
        || optional_id(&row, "to_location_id")?.is_some()
        || row
            .try_get::<Option<i64>, _>("to_location_revision")
            .map_err(|_| corrupt_storage())?
            .is_some()
        || stock::read::occurrence(&row)? != Occurrence::Unknown
    {
        return Err(corrupt_storage());
    }
    Ok(())
}

pub(super) async fn single_event_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
) -> Result<(), SafeError> {
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_stock_events WHERE operation_id=?")
            .bind(operation.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| storage_unavailable())?;
    if count != 1 {
        return Err(corrupt_storage());
    }
    Ok(())
}
