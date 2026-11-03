use super::*;
use harvestcircle_domain::hub::movement::WorkProgress;
impl Database {
    pub async fn apply_hub_disposition(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: DispositionCommand,
        now: UnixTimestamp,
    ) -> Result<HubDispositionReceipt, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move { apply_on(tx, actor, operation, command, now).await })
            })
            .await
            .map_err(map_transaction_error)
    }
    pub async fn hub_disposition_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubDispositionReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    match stock::read::operation_on(tx, hub.id(), &operation).await? {
                        None => Ok(None),
                        Some(op) => Ok(Some(read::original_on(tx, hub.id(), op).await?)),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
pub(super) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: DispositionCommand,
    now: UnixTimestamp,
) -> Result<HubDispositionReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let ids = identities(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, ids);
    if let Some(original) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if original.kind != "disposition" || original.actor != actor || original.hash != hash {
            return Err(stock::conflict());
        }
        return read::original_on(tx, hub.id(), original).await;
    }
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::Ordinary(1),
    )
    .await?;
    let context = context::load_on(tx, hub.id(), &operation, &command).await?;
    let plan = command
        .plan(&context.domain(hub.id()), ids, actor, now)
        .map_err(error)?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "disposition".into(),
        actor,
        hash,
        catalog_revision: plan.catalog_revision,
        revision: plan.stock_revision,
        result: ids.disposition,
        recorded_at: now,
    };
    handling::write::insert_operation_on(tx, hub.id(), &op).await?;
    capture_on(tx, &operation, &plan, &hash).await?;
    let group = &plan.consumed.before;
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
        return Err(stock::conflict());
    }
    let changed =
        sqlx::query("UPDATE hub_stock_groups SET revision=? WHERE group_id=? AND revision=?")
            .bind(integer(plan.consumed.after_revision)?)
            .bind(group.id.as_bytes().as_slice())
            .bind(integer(group.revision)?)
            .execute(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    let retired=sqlx::query("UPDATE hub_group_work SET progress='no_longer_required' WHERE group_id=? AND progress='pending'").bind(group.id.as_bytes().as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if retired.rows_affected()
        != u64::try_from(
            group
                .work
                .iter()
                .filter(|a| a.progress == WorkProgress::Pending)
                .count(),
        )
        .map_err(|_| corrupt_storage())?
    {
        return Err(stock::conflict());
    }
    for change in &plan.work_changes {
        handling::write::update_work_on(tx, &change.before, &change.after).await?;
        handling::write::work_change_on(tx, &operation, Some(&change.before), &change.after)
            .await?;
    }
    handling::write::event_on(
        tx,
        &operation,
        handling::write::EventEffect {
            kind: "disposition",
            prior_events: context.state.event_count,
            trace: group.trace,
            group: group.id,
            packs: group.packs,
            from: Some(&plan.location),
            to: None,
            occurred: plan.occurred,
        },
    )
    .await?;
    handling::write::advance_many_on(tx, hub.id(), &context.state, plan.stock_revision, 1).await?;
    let restored = read::original_on(tx, hub.id(), op).await?;
    if restored.original != plan {
        return Err(corrupt_storage());
    }
    let loaded = handling::lineage::load_on(
        tx,
        hub.id(),
        group.trace,
        &context.catalog,
        plan.stock_revision,
    )
    .await?;
    let node = loaded
        .rows
        .iter()
        .find(|r| r.id == group.id)
        .ok_or_else(corrupt_storage)?;
    if node.packs.value() != 0 || node.revision != plan.consumed.after_revision {
        return Err(corrupt_storage());
    }
    Ok(restored)
}
async fn capture_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    plan: &DispositionRecord,
    input: &[u8; 32],
) -> Result<(), SafeError> {
    let group = &plan.consumed.before;
    let (kind, start, end) = stock::write::occurrence_fields(plan.occurred);
    let disposition = match plan.kind {
        DispositionKind::Destroyed => "destroyed",
        DispositionKind::OtherObservedRemoval => "other_observed_removal",
    };
    let result = disposition_result_digest(input, plan).map_err(|_| corrupt_storage())?;
    sqlx::query("INSERT INTO hub_dispositions (operation_id,disposition_id,policy_version,group_id,trace_id,before_group_revision,after_group_revision,packs,location_id,location_revision,before_causal_floor_unix_s,after_causal_floor_unix_s,scope_count,work_change_count,disposition_kind,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,observation,result_hash) VALUES (?,?,1,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(operation.as_str()).bind(plan.identities.disposition.as_bytes().as_slice()).bind(group.id.as_bytes().as_slice()).bind(group.trace.as_bytes().as_slice()).bind(integer(group.revision)?).bind(integer(plan.consumed.after_revision)?).bind(i64::from(group.packs.value())).bind(group.location.as_bytes().as_slice()).bind(integer(plan.location.revision)?)
        .bind(group.causal_floor.map(|t|t.as_seconds())).bind(plan.consumed.causal_floor.map(|t|t.as_seconds())).bind(i64::try_from(group.work.len()).map_err(|_|corrupt_storage())?).bind(i64::try_from(plan.work_changes.len()).map_err(|_|corrupt_storage())?)
        .bind(disposition).bind(kind).bind(start).bind(end).bind(plan.observation.as_str()).bind(result.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    for allocation in &group.work {
        let work = &allocation.requirement;
        let progress = match allocation.progress {
            WorkProgress::Pending => "pending",
            WorkProgress::Completed => "completed",
            WorkProgress::NoLongerRequired => "no_longer_required",
        };
        sqlx::query("INSERT INTO hub_disposition_work_scope (operation_id,work_id,before_progress,before_work_revision,before_completed,before_no_longer_required,before_remaining) VALUES (?,?,?,?,?,?,?)")
            .bind(operation.as_str()).bind(work.id.as_bytes().as_slice()).bind(progress).bind(integer(work.revision)?).bind(i64::from(work.quantity.completed().value())).bind(i64::from(work.quantity.no_longer_required().value())).bind(i64::from(work.quantity.remaining().value())).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    Ok(())
}
