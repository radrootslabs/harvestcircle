use super::super::{reference, stock};
use super::*;
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::movement::{HUB_REQUIREMENT_GROUPS, MovementCommand, WorkProgress};
use harvestcircle_domain::hub::reference::{ReferenceCatalog, ReferenceKind};
use harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY;

impl Database {
    pub async fn apply_hub_movement(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: MovementCommand,
        now: UnixTimestamp,
    ) -> Result<HubHandlingReceipt, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move {
                    crate::hub::handling::treatment::capacity::ensure_on(
                        transaction,
                        actor,
                        &operation,
                        crate::hub::handling::treatment::capacity::Reservation::Ordinary(1),
                    )
                    .await?;
                    apply_movement_on(transaction, actor, operation, command, now).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn apply_movement_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: MovementCommand,
    now: UnixTimestamp,
) -> Result<HubHandlingReceipt, SafeError> {
    let hub = reference::authorize_on(transaction, actor).await?;
    reference::validate_basis_on(transaction, hub.id()).await?;
    let ids = handling_ids(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, ids.group);
    if let Some(original) = stock::read::operation_on(transaction, hub.id(), &operation).await? {
        if original.kind != "move" || original.actor != actor || original.hash != hash {
            return Err(conflict());
        }
        return history::movement_on(transaction, hub.id(), original).await;
    }
    let state = stock::read::state_on(transaction, hub.id()).await?;
    let catalog = reference::load_catalog_on(transaction, hub.id()).await?;
    let source = read::group_on(transaction, hub.id(), command.group, &catalog).await?;
    let plan = command
        .plan(&catalog, state.revision, &source, ids.group, now)
        .map_err(handling_error)?;
    if state.event_count as usize >= HUB_STOCK_EVENT_CAPACITY {
        return Err(capacity());
    }
    if plan.remaining.is_some() {
        ensure_portion_capacity_on(
            transaction,
            &source,
            plan.moved
                .identification
                .as_ref()
                .ok_or_else(corrupt_storage)?,
        )
        .await?;
    }
    let recorded = stock::read::OperationRecord {
        id: operation.clone(),
        kind: "move".to_owned(),
        actor,
        hash,
        catalog_revision: command.expected_catalog_revision,
        revision: plan.stock_revision,
        result: plan.moved.id,
        recorded_at: now,
    };
    insert_operation_on(transaction, hub.id(), &recorded).await?;
    write_groups_on(
        transaction,
        &catalog,
        &operation,
        &source,
        GroupChange {
            remaining: plan.remaining.as_ref(),
            selected: &plan.moved,
            destination: &plan.destination,
        },
    )
    .await?;
    let mut work_changes = Vec::with_capacity(plan.work_updates.len());
    for after in &plan.work_updates {
        let before = source
            .work
            .iter()
            .find(|allocation| allocation.requirement.id == after.id)
            .ok_or_else(corrupt_storage)?
            .requirement
            .clone();
        update_work_on(transaction, &before, after).await?;
        work_change_on(transaction, &operation, Some(&before), after).await?;
        work_changes.push(HubWorkChange {
            before: Some(before),
            after: after.clone(),
        });
    }
    sqlx::query("INSERT INTO hub_movement_details (operation_id,selected_group_id,expected_group_revision,source_before_packs,moved_group_id,work_change_count,observation) VALUES (?,?,?,?,?,?,?)")
        .bind(operation.as_str()).bind(source.id.as_bytes().as_slice()).bind(integer(source.revision)?).bind(i64::from(source.packs.value()))
        .bind(plan.moved.id.as_bytes().as_slice()).bind(i64::try_from(work_changes.len()).map_err(|_|corrupt_storage())?).bind(command.observation.as_str()).execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    let from = catalog
        .require_kind(source.location, ReferenceKind::Location)
        .map_err(|_| corrupt_storage())?
        .clone();
    event_on(
        transaction,
        &operation,
        EventEffect {
            kind: "move",
            prior_events: state.event_count,
            trace: source.trace,
            group: plan.moved.id,
            packs: command.packs,
            from: Some(&from),
            to: Some(&plan.destination),
            occurred: command.occurred,
        },
    )
    .await?;
    advance_on(transaction, hub.id(), &state, plan.stock_revision).await?;
    // Verify the complete committed projection before the protected transaction exits.
    let actual = read::group_on(transaction, hub.id(), plan.moved.id, &catalog).await?;
    if actual != plan.moved {
        return Err(corrupt_storage());
    }
    if let Some(remaining) = &plan.remaining
        && read::group_on(transaction, hub.id(), remaining.id, &catalog).await? != *remaining
    {
        return Err(corrupt_storage());
    }
    Ok(HubHandlingReceipt {
        operation,
        kind: HubHandlingKind::Move,
        revision: plan.stock_revision,
        catalog_revision: catalog.revision(),
        trace: source.trace,
        actor,
        recorded_at: now,
        result: HubHandlingResult::Moved(HubMovementResult {
            selected_group: source.id,
            expected_group_revision: source.revision,
            source_before_packs: source.packs,
            from,
            to: plan.destination,
            packs: command.packs,
            moved_group: plan.moved.id,
            remaining_packs: source
                .packs
                .checked_sub(command.packs)
                .map_err(|_| corrupt_storage())?,
            separated_portion: command.separated_portion,
            occurred: command.occurred,
            observation: command.observation,
            work_changes,
        }),
    })
}

pub(super) struct GroupChange<'a> {
    pub remaining: Option<&'a PhysicalStockGroup>,
    pub selected: &'a PhysicalStockGroup,
    pub destination: &'a ReferenceRecord,
}

pub(super) async fn write_groups_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    catalog: &ReferenceCatalog,
    operation: &DurableRequestId,
    source: &PhysicalStockGroup,
    plan: GroupChange<'_>,
) -> Result<(), SafeError> {
    let after = plan.remaining.unwrap_or(plan.selected);
    let changed =
        sqlx::query("UPDATE hub_stock_groups SET revision=? WHERE group_id=? AND revision=?")
            .bind(integer(after.revision)?)
            .bind(source.id.as_bytes().as_slice())
            .bind(integer(source.revision)?)
            .execute(&mut *transaction)
            .await
            .map_err(|_| storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(conflict());
    }
    let remaining = plan.remaining.map_or(0, |group| group.packs.value());
    let from = catalog
        .require_kind(source.location, ReferenceKind::Location)
        .map_err(|_| corrupt_storage())?;
    let changed=sqlx::query("UPDATE hub_stock_positions SET packs=?,revision=revision+1,location_revision=? WHERE group_id=? AND location_id=? AND packs=?")
        .bind(i64::from(remaining)).bind(integer(from.revision)?).bind(source.id.as_bytes().as_slice()).bind(source.location.as_bytes().as_slice()).bind(i64::from(source.packs.value()))
        .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(conflict());
    }
    if plan.remaining.is_some() {
        let identity = plan
            .selected
            .identification
            .as_ref()
            .ok_or_else(corrupt_storage)?;
        sqlx::query("INSERT INTO hub_stock_groups (group_id,trace_id,revision) VALUES (?,?,1)")
            .bind(plan.selected.id.as_bytes().as_slice())
            .bind(source.trace.as_bytes().as_slice())
            .execute(&mut *transaction)
            .await
            .map_err(|_| storage_unavailable())?;
        sqlx::query("INSERT INTO hub_group_identification (group_id,parent_group_id,physical_label,observation,created_operation_id) VALUES (?,?,?,?,?)")
            .bind(plan.selected.id.as_bytes().as_slice()).bind(source.id.as_bytes().as_slice()).bind(identity.label.as_str()).bind(identity.observation.as_str()).bind(operation.as_str())
            .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
        for allocation in &plan.selected.work {
            let changed=sqlx::query("UPDATE hub_group_work SET scoped_packs=? WHERE group_id=? AND work_id=? AND scoped_packs=?")
                .bind(i64::from(remaining)).bind(source.id.as_bytes().as_slice()).bind(allocation.requirement.id.as_bytes().as_slice()).bind(i64::from(source.packs.value()))
                .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
            if changed.rows_affected() != 1 {
                return Err(conflict());
            }
            sqlx::query("INSERT INTO hub_group_work (group_id,work_id,progress,scoped_packs) VALUES (?,?,?,?)")
                .bind(plan.selected.id.as_bytes().as_slice()).bind(allocation.requirement.id.as_bytes().as_slice()).bind(progress(allocation.progress)).bind(i64::from(plan.selected.packs.value()))
                .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
        }
    } else {
        for (before, after) in source.work.iter().zip(&plan.selected.work) {
            if before.progress != after.progress {
                let changed=sqlx::query("UPDATE hub_group_work SET progress=? WHERE group_id=? AND work_id=? AND progress=? AND scoped_packs=?")
                    .bind(progress(after.progress)).bind(source.id.as_bytes().as_slice()).bind(after.requirement.id.as_bytes().as_slice()).bind(progress(before.progress)).bind(i64::from(source.packs.value()))
                    .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
                if changed.rows_affected() != 1 {
                    return Err(conflict());
                }
            }
        }
    }
    let changed=sqlx::query("INSERT INTO hub_stock_positions (group_id,location_id,location_revision,packs,revision) VALUES (?,?,?,?,1) ON CONFLICT(group_id,location_id) DO UPDATE SET location_revision=excluded.location_revision,packs=excluded.packs,revision=hub_stock_positions.revision+1 WHERE hub_stock_positions.packs=0")
        .bind(plan.selected.id.as_bytes().as_slice()).bind(plan.destination.id.as_bytes().as_slice()).bind(integer(plan.destination.revision)?).bind(i64::from(plan.selected.packs.value()))
        .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(conflict());
    }
    Ok(())
}

pub(in crate::hub) async fn insert_operation_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &stock::read::OperationRecord,
) -> Result<(), SafeError> {
    sqlx::query("INSERT INTO hub_stock_operations (operation_id,hub_id,actor_public_key,kind,intent_hash,catalog_revision,stock_revision,result_id,recorded_at_unix_s) VALUES (?,?,?,?,?,?,?,?,?)")
        .bind(operation.id.as_str()).bind(hub.as_bytes().as_slice()).bind(operation.actor.as_bytes().as_slice()).bind(&operation.kind).bind(operation.hash.as_slice()).bind(integer(operation.catalog_revision)?).bind(integer(operation.revision)?).bind(operation.result.as_bytes().as_slice()).bind(operation.recorded_at.as_seconds())
        .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    Ok(())
}

pub(super) async fn update_work_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    before: &WorkRequirement,
    after: &WorkRequirement,
) -> Result<(), SafeError> {
    let changed=sqlx::query("UPDATE hub_work_requirements SET completed=?,no_longer_required=?,remaining=?,revision=? WHERE work_id=? AND revision=? AND completed=? AND no_longer_required=? AND remaining=?")
        .bind(i64::from(after.quantity.completed().value())).bind(i64::from(after.quantity.no_longer_required().value())).bind(i64::from(after.quantity.remaining().value())).bind(integer(after.revision)?)
        .bind(before.id.as_bytes().as_slice()).bind(integer(before.revision)?).bind(i64::from(before.quantity.completed().value())).bind(i64::from(before.quantity.no_longer_required().value())).bind(i64::from(before.quantity.remaining().value()))
        .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(conflict());
    }
    Ok(())
}

pub(super) async fn work_change_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    before: Option<&WorkRequirement>,
    after: &WorkRequirement,
) -> Result<(), SafeError> {
    sqlx::query("INSERT INTO hub_work_changes (operation_id,work_id,before_completed,before_no_longer_required,before_remaining,before_revision,after_completed,after_no_longer_required,after_remaining,after_revision) VALUES (?,?,?,?,?,?,?,?,?,?)")
        .bind(operation.as_str()).bind(after.id.as_bytes().as_slice()).bind(before.map(|work|i64::from(work.quantity.completed().value())))
        .bind(before.map(|work|i64::from(work.quantity.no_longer_required().value()))).bind(before.map(|work|i64::from(work.quantity.remaining().value())))
        .bind(before.map(|work|integer(work.revision)).transpose()?).bind(i64::from(after.quantity.completed().value())).bind(i64::from(after.quantity.no_longer_required().value()))
        .bind(i64::from(after.quantity.remaining().value())).bind(integer(after.revision)?).execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    Ok(())
}

pub(in crate::hub) struct EventEffect<'a> {
    pub kind: &'static str,
    pub prior_events: u32,
    pub trace: HubRecordId,
    pub group: HubRecordId,
    pub packs: WholePacks,
    pub from: Option<&'a ReferenceRecord>,
    pub to: Option<&'a ReferenceRecord>,
    pub occurred: Occurrence,
}

pub(in crate::hub) async fn event_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    effect: EventEffect<'_>,
) -> Result<(), SafeError> {
    let (occurrence, start, end) = occurrence_fields(effect.occurred);
    sqlx::query("INSERT INTO hub_stock_events (sequence,operation_id,kind,trace_id,group_id,packs,from_location_id,from_location_revision,to_location_id,to_location_revision,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(i64::from(effect.prior_events)+1).bind(operation.as_str()).bind(effect.kind).bind(effect.trace.as_bytes().as_slice()).bind(effect.group.as_bytes().as_slice()).bind(i64::from(effect.packs.value()))
        .bind(effect.from.map(|record|record.id.as_bytes().as_slice())).bind(effect.from.map(|record|integer(record.revision)).transpose()?)
        .bind(effect.to.map(|record|record.id.as_bytes().as_slice())).bind(effect.to.map(|record|integer(record.revision)).transpose()?)
        .bind(occurrence).bind(start).bind(end).execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    Ok(())
}

pub(super) async fn advance_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    state: &stock::read::StockState,
    revision: HubRevision,
) -> Result<(), SafeError> {
    advance_many_on(transaction, hub, state, revision, 1).await
}

pub(in crate::hub) async fn advance_many_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    state: &stock::read::StockState,
    revision: HubRevision,
    events: u32,
) -> Result<(), SafeError> {
    if events == 0
        || events as usize > harvestcircle_domain::hub::dispatch::HUB_DISPATCH_LINES
        || state.revision.value().checked_add(1) != Some(revision.value())
    {
        return Err(corrupt_storage());
    }
    let count = state.event_count.checked_add(events).ok_or_else(capacity)?;
    if count as usize > HUB_STOCK_EVENT_CAPACITY {
        return Err(capacity());
    }
    let changed=sqlx::query("UPDATE hub_stock_state SET revision=?,event_count=? WHERE singleton=1 AND hub_id=? AND revision=? AND event_count=?")
        .bind(integer(revision)?).bind(i64::from(count)).bind(hub.as_bytes().as_slice()).bind(integer(state.revision)?).bind(i64::from(state.event_count))
        .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(conflict());
    }
    Ok(())
}

pub(super) const fn progress(progress: WorkProgress) -> &'static str {
    match progress {
        WorkProgress::Pending => "pending",
        WorkProgress::Completed => "completed",
        WorkProgress::NoLongerRequired => "no_longer_required",
    }
}

pub(super) async fn ensure_portion_capacity_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    source: &PhysicalStockGroup,
    identity: &PhysicalIdentification,
) -> Result<(), SafeError> {
    let groups: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_stock_groups")
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    if groups >= HUB_GROUP_CAPACITY as i64 {
        return Err(capacity());
    }
    let in_use:i64=sqlx::query_scalar("SELECT count(*) FROM hub_group_identification i WHERE i.physical_label=? AND EXISTS(SELECT 1 FROM hub_stock_positions p WHERE p.group_id=i.group_id AND p.packs>0)")
            .bind(identity.label.as_str()).fetch_one(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if in_use != 0 {
        return Err(conflict());
    }
    for allocation in &source.work {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_group_work WHERE work_id=?")
            .bind(allocation.requirement.id.as_bytes().as_slice())
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| storage_unavailable())?;
        if count >= HUB_REQUIREMENT_GROUPS as i64 {
            return Err(capacity());
        }
    }
    Ok(())
}
