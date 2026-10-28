use super::super::{reference, stock};
use super::*;
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::movement::{HUB_REQUIREMENT_GROUPS, NewWorkCommand, ReplanCommand};
use harvestcircle_domain::hub::reference::{ReferenceCatalog, ReferenceKind, SourceProvenance};
use harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY;
use stock::read::{OperationRecord, StockState, record_id};

impl Database {
    pub async fn apply_hub_replan(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: ReplanCommand,
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
                    apply_replan_on(transaction, actor, operation, command, now).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
    pub async fn apply_hub_new_work(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: NewWorkCommand,
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
                    apply_new_work_on(transaction, actor, operation, command, now).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn apply_replan_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: ReplanCommand,
    now: UnixTimestamp,
) -> Result<HubHandlingReceipt, SafeError> {
    let hub = reference::authorize_on(transaction, actor).await?;
    reference::validate_basis_on(transaction, hub.id()).await?;
    let ids = handling_ids(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, ids.requirement);
    if let Some(original) = stock::read::operation_on(transaction, hub.id(), &operation).await? {
        if original.kind != "work_replan" || original.actor != actor || original.hash != hash {
            return Err(conflict());
        }
        return history::work_on(transaction, hub.id(), original).await;
    }
    let state = stock::read::state_on(transaction, hub.id()).await?;
    let catalog = reference::load_catalog_on(transaction, hub.id()).await?;
    let prior = read::work_on(transaction, hub.id(), command.requirement).await?;
    let selected=sqlx::query("SELECT substr(group_id,1,17) AS group_id FROM hub_group_work WHERE work_id=? AND progress='pending' AND scoped_packs>0 ORDER BY group_id LIMIT 257")
        .bind(prior.id.as_bytes().as_slice()).fetch_all(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    if selected.len() > HUB_REQUIREMENT_GROUPS {
        return Err(corrupt_storage());
    }
    let mut groups = Vec::with_capacity(selected.len());
    for row in selected {
        groups.push(
            read::group_on(
                transaction,
                hub.id(),
                record_id(&row, "group_id")?,
                &catalog,
            )
            .await?,
        );
    }
    let plan = command
        .plan(&catalog, state.revision, &prior, &groups, ids.requirement)
        .map_err(handling_error)?;
    ensure_capacity_on(transaction, &state, ids.requirement).await?;
    let recorded = OperationRecord {
        id: operation.clone(),
        kind: "work_replan".to_owned(),
        actor,
        hash,
        catalog_revision: command.expected_catalog_revision,
        revision: plan.stock_revision,
        result: plan.replacement.id,
        recorded_at: now,
    };
    write::insert_operation_on(transaction, hub.id(), &recorded).await?;
    insert_requirement_on(transaction, &operation, &plan.replacement).await?;
    write::update_work_on(transaction, &prior, &plan.previous).await?;
    write::work_change_on(transaction, &operation, Some(&prior), &plan.previous).await?;
    write::work_change_on(transaction, &operation, None, &plan.replacement).await?;
    for (before, after) in groups.iter().zip(&plan.groups) {
        update_group_revision_on(transaction, before, after).await?;
        let changed=sqlx::query("UPDATE hub_group_work SET progress='no_longer_required' WHERE group_id=? AND work_id=? AND progress='pending' AND scoped_packs=?")
            .bind(before.id.as_bytes().as_slice()).bind(prior.id.as_bytes().as_slice()).bind(i64::from(before.packs.value())).execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
        if changed.rows_affected() != 1 {
            return Err(conflict());
        }
        insert_allocation_on(transaction, before, &plan.replacement).await?;
    }
    let captured = capture_groups(&catalog, &groups)?;
    record_action_on(
        transaction,
        &operation,
        Some(&prior),
        &plan.replacement,
        &captured,
        &command.observation,
    )
    .await?;
    record_work_event_on(transaction, &recorded, &state, &plan.replacement).await?;
    write::advance_on(transaction, hub.id(), &state, plan.stock_revision).await?;
    verify_groups_on(transaction, hub.id(), &catalog, &plan.groups).await?;
    Ok(HubHandlingReceipt {
        operation,
        kind: HubHandlingKind::Replan,
        revision: plan.stock_revision,
        catalog_revision: catalog.revision(),
        trace: prior.trace,
        actor,
        recorded_at: now,
        result: HubHandlingResult::WorkChanged(HubWorkActionResult {
            replacement: plan.replacement,
            previous: Some(HubWorkChange {
                before: Some(prior),
                after: plan.previous,
            }),
            groups: captured,
            observation: command.observation,
        }),
    })
}

pub(super) async fn apply_new_work_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: NewWorkCommand,
    now: UnixTimestamp,
) -> Result<HubHandlingReceipt, SafeError> {
    let hub = reference::authorize_on(transaction, actor).await?;
    reference::validate_basis_on(transaction, hub.id()).await?;
    command.validate_shape().map_err(handling_error)?;
    let ids = handling_ids(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, ids.requirement);
    if let Some(original) = stock::read::operation_on(transaction, hub.id(), &operation).await? {
        if original.kind != "work_new" || original.actor != actor || original.hash != hash {
            return Err(conflict());
        }
        return history::work_on(transaction, hub.id(), original).await;
    }
    let state = stock::read::state_on(transaction, hub.id()).await?;
    let catalog = reference::load_catalog_on(transaction, hub.id()).await?;
    let mut groups = Vec::with_capacity(command.groups.len());
    for id in &command.groups {
        groups.push(read::group_on(transaction, hub.id(), *id, &catalog).await?);
    }
    let plan = command
        .plan(&catalog, state.revision, &groups, ids.requirement)
        .map_err(handling_error)?;
    ensure_capacity_on(transaction, &state, ids.requirement).await?;
    let recorded = OperationRecord {
        id: operation.clone(),
        kind: "work_new".to_owned(),
        actor,
        hash,
        catalog_revision: command.expected_catalog_revision,
        revision: plan.stock_revision,
        result: plan.requirement.id,
        recorded_at: now,
    };
    write::insert_operation_on(transaction, hub.id(), &recorded).await?;
    insert_requirement_on(transaction, &operation, &plan.requirement).await?;
    write::work_change_on(transaction, &operation, None, &plan.requirement).await?;
    for (before, after) in groups.iter().zip(&plan.groups) {
        update_group_revision_on(transaction, before, after).await?;
        insert_allocation_on(transaction, before, &plan.requirement).await?;
    }
    let captured = capture_groups(&catalog, &groups)?;
    record_action_on(
        transaction,
        &operation,
        None,
        &plan.requirement,
        &captured,
        &command.observation,
    )
    .await?;
    record_work_event_on(transaction, &recorded, &state, &plan.requirement).await?;
    write::advance_on(transaction, hub.id(), &state, plan.stock_revision).await?;
    verify_groups_on(transaction, hub.id(), &catalog, &plan.groups).await?;
    Ok(HubHandlingReceipt {
        operation,
        kind: HubHandlingKind::NewWork,
        revision: plan.stock_revision,
        catalog_revision: catalog.revision(),
        trace: plan.requirement.trace,
        actor,
        recorded_at: now,
        result: HubHandlingResult::WorkChanged(HubWorkActionResult {
            replacement: plan.requirement,
            previous: None,
            groups: captured,
            observation: command.observation,
        }),
    })
}

async fn ensure_capacity_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    state: &StockState,
    id: HubRecordId,
) -> Result<(), SafeError> {
    if state.event_count as usize >= HUB_STOCK_EVENT_CAPACITY {
        return Err(capacity());
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_work_requirements")
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| storage_unavailable())?;
    if count >= HUB_WORK_CAPACITY as i64 {
        return Err(capacity());
    }
    let exists: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_work_requirements WHERE work_id=?")
            .bind(id.as_bytes().as_slice())
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| storage_unavailable())?;
    if exists != 0 {
        return Err(conflict());
    }
    Ok(())
}

async fn insert_requirement_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    work: &WorkRequirement,
) -> Result<(), SafeError> {
    sqlx::query("INSERT INTO hub_work_requirements (work_id,trace_id,variant_id,target_location_id,target_location_revision,required,completed,no_longer_required,remaining,revision,created_operation_id) VALUES (?,?,?,?,?,?,0,0,?,1,?)")
        .bind(work.id.as_bytes().as_slice()).bind(work.trace.as_bytes().as_slice()).bind(work.variant.as_bytes().as_slice()).bind(work.target.as_ref().map(|target|target.id.as_bytes().as_slice()))
        .bind(work.target.as_ref().map(|target|integer(target.revision)).transpose()?).bind(i64::from(work.quantity.required().value())).bind(i64::from(work.quantity.remaining().value())).bind(operation.as_str())
        .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    Ok(())
}

async fn insert_allocation_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    group: &PhysicalStockGroup,
    work: &WorkRequirement,
) -> Result<(), SafeError> {
    sqlx::query("INSERT INTO hub_group_work (group_id,work_id,progress,scoped_packs) VALUES (?,?,'pending',?)")
        .bind(group.id.as_bytes().as_slice()).bind(work.id.as_bytes().as_slice()).bind(i64::from(group.packs.value())).execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    Ok(())
}

async fn update_group_revision_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    before: &PhysicalStockGroup,
    after: &PhysicalStockGroup,
) -> Result<(), SafeError> {
    let changed =
        sqlx::query("UPDATE hub_stock_groups SET revision=? WHERE group_id=? AND revision=?")
            .bind(integer(after.revision)?)
            .bind(before.id.as_bytes().as_slice())
            .bind(integer(before.revision)?)
            .execute(&mut *transaction)
            .await
            .map_err(|_| storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(conflict());
    }
    Ok(())
}

fn capture_groups(
    catalog: &ReferenceCatalog,
    groups: &[PhysicalStockGroup],
) -> Result<Vec<HubActionGroup>, SafeError> {
    groups
        .iter()
        .map(|group| {
            Ok(HubActionGroup {
                group: group.id,
                before_revision: group.revision,
                packs: group.packs,
                location: catalog
                    .require_kind(group.location, ReferenceKind::Location)
                    .map_err(|_| corrupt_storage())?
                    .clone(),
            })
        })
        .collect()
}

async fn record_action_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    previous: Option<&WorkRequirement>,
    replacement: &WorkRequirement,
    groups: &[HubActionGroup],
    observation: &SourceProvenance,
) -> Result<(), SafeError> {
    sqlx::query("INSERT INTO hub_work_actions (operation_id,previous_work_id,expected_work_revision,replacement_work_id,observation) VALUES (?,?,?,?,?)")
        .bind(operation.as_str()).bind(previous.map(|work|work.id.as_bytes().as_slice())).bind(previous.map(|work|integer(work.revision)).transpose()?).bind(replacement.id.as_bytes().as_slice()).bind(observation.as_str())
        .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    for group in groups {
        sqlx::query("INSERT INTO hub_action_groups (operation_id,group_id,before_revision,scoped_packs,location_id,location_revision) VALUES (?,?,?,?,?,?)")
            .bind(operation.as_str()).bind(group.group.as_bytes().as_slice()).bind(integer(group.before_revision)?).bind(i64::from(group.packs.value()))
            .bind(group.location.id.as_bytes().as_slice()).bind(integer(group.location.revision)?).execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    }
    Ok(())
}

async fn record_work_event_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    recorded: &OperationRecord,
    state: &StockState,
    work: &WorkRequirement,
) -> Result<(), SafeError> {
    let row = sqlx::query(
        "SELECT substr(root_group_id,1,17) AS group_id FROM hub_receipts WHERE trace_id=? LIMIT 1",
    )
    .bind(work.trace.as_bytes().as_slice())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| storage_unavailable())?
    .ok_or_else(corrupt_storage)?;
    let kind = match recorded.kind.as_str() {
        "work_replan" => "work_replan",
        "work_new" => "work_new",
        _ => return Err(corrupt_storage()),
    };
    write::event_on(
        transaction,
        &recorded.id,
        write::EventEffect {
            kind,
            prior_events: state.event_count,
            trace: work.trace,
            group: record_id(&row, "group_id")?,
            packs: WholePacks::from_value(0).map_err(|_| corrupt_storage())?,
            from: None,
            to: None,
            occurred: Occurrence::Unknown,
        },
    )
    .await
}

async fn verify_groups_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    catalog: &ReferenceCatalog,
    groups: &[PhysicalStockGroup],
) -> Result<(), SafeError> {
    for group in groups {
        if read::group_on(transaction, hub, group.id, catalog).await? != *group {
            return Err(corrupt_storage());
        }
    }
    Ok(())
}
