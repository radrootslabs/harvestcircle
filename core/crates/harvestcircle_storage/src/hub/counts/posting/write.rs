//! The protected transaction commits the result, physical/work effects and history together.
use super::*;

pub(in crate::hub) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: DurableRequestId,
    command: CountPostCommand,
    hash: [u8; 32],
    now: UnixTimestamp,
) -> Result<HubCountPostReceipt, SafeError> {
    let facts = super::super::review::load_on(tx, hub, command.count, Some(&operation)).await?;
    let plan = command
        .plan(
            &facts.observation,
            &facts.context(),
            operation_id(&operation)?,
            actor,
            now,
        )
        .map_err(count_error)?;
    let surplus = plan.surplus().map_err(count_error)?;
    let events = 1 + u32::from(surplus.is_some());
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::Ordinary(events),
    )
    .await?;
    if let Some(new) = &surplus {
        ensure_surplus_on(tx, new).await?;
    }
    let record = stock::read::OperationRecord {
        id: operation.clone(),
        kind: "count_post".into(),
        actor,
        hash,
        catalog_revision: plan.catalog_revision,
        revision: plan.stock_revision,
        result: plan.id,
        recorded_at: now,
    };
    handling::write::insert_operation_on(tx, hub, &record).await?;
    let (kind, changed, after_packs, after_revision, changes) = match &plan.effect {
        CountEffect::NoVariance => ("none", 0, plan.before.packs, plan.before.revision, 0),
        CountEffect::Shortage {
            packs,
            after_packs,
            after_revision,
            work_changes,
        } => (
            "shortage",
            packs.value(),
            *after_packs,
            *after_revision,
            work_changes.len(),
        ),
        CountEffect::Surplus { packs, .. } => (
            "surplus",
            packs.value(),
            plan.before.packs,
            plan.before.revision,
            0,
        ),
    };
    let digest = count_post_result_digest(&hash, &plan).map_err(|_| corrupt_storage())?;
    sqlx::query(concat!(
        "INSERT INTO hub_count_reconciliations (reconciliation_id,operation_id,count_id,policy_version,effect_kind,changed_packs,after_on_hand_packs,after_group_revision,before_causal_floor_unix_s,",
        "confirmed_on_hand_at_unix_s,scope_work_count,changed_work_count,surplus_population_id,reason,result_hash) VALUES (?,?,?,1,?,?,?,?,?,?,?,?,?,?,?)"
    )).bind(plan.id.as_bytes().as_slice()).bind(operation.as_str()).bind(plan.count.id.as_bytes().as_slice())
        .bind(kind).bind(i64::from(changed)).bind(i64::from(after_packs.value())).bind(integer(after_revision)?)
        .bind(plan.before.causal_floor.map(|t|t.as_seconds())).bind(plan.confirmed_on_hand_at.as_seconds())
        .bind(i64::try_from(plan.before.work.len()).map_err(|_|corrupt_storage())?).bind(i64::try_from(changes).map_err(|_|corrupt_storage())?)
        .bind(surplus.as_ref().map(|s|s.population.id.as_bytes().as_slice())).bind(plan.reason.as_str()).bind(digest.as_slice())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    // Capture every original allocation/counter before changing any mutable row.
    for (ordinal, allocation) in plan.before.work.iter().enumerate() {
        let before = &allocation.requirement;
        let change = match &plan.effect {
            CountEffect::Shortage { work_changes, .. } => {
                work_changes.iter().find(|c| c.before.id == before.id)
            }
            _ => None,
        };
        let after = change.map(|c| &c.after).unwrap_or(before);
        let removed = if kind == "shortage" { changed } else { 0 };
        let removed_progress = if removed == 0 {
            None
        } else {
            Some(match allocation.progress {
                WorkProgress::Pending => WorkProgress::NoLongerRequired,
                p => p,
            })
        };
        sqlx::query(concat!(
            "INSERT INTO hub_count_reconciliation_work (reconciliation_id,ordinal,work_id,before_progress,before_scoped_packs,on_hand_after,removed_packs,removed_progress,required,",
            "before_completed,before_no_longer_required,before_remaining,before_revision,after_completed,after_no_longer_required,after_remaining,after_revision) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"
        )).bind(plan.id.as_bytes().as_slice()).bind(i64::try_from(ordinal).map_err(|_|corrupt_storage())?).bind(before.id.as_bytes().as_slice()).bind(progress(allocation.progress))
            .bind(i64::from(plan.before.packs.value())).bind(i64::from(after_packs.value())).bind(i64::from(removed)).bind(removed_progress.map(progress))
            .bind(i64::from(before.quantity.required().value())).bind(i64::from(before.quantity.completed().value())).bind(i64::from(before.quantity.no_longer_required().value()))
            .bind(i64::from(before.quantity.remaining().value())).bind(integer(before.revision)?)
            .bind(i64::from(after.quantity.completed().value())).bind(i64::from(after.quantity.no_longer_required().value())).bind(i64::from(after.quantity.remaining().value())).bind(integer(after.revision)?)
            .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    if let CountEffect::Shortage { work_changes, .. } = &plan.effect {
        for change in work_changes {
            handling::write::update_work_on(tx, &change.before, &change.after).await?;
        }
        let changed_group =
            sqlx::query("UPDATE hub_stock_groups SET revision=? WHERE group_id=? AND revision=?")
                .bind(integer(after_revision)?)
                .bind(plan.before.id.as_bytes().as_slice())
                .bind(integer(plan.before.revision)?)
                .execute(&mut *tx)
                .await
                .map_err(|_| storage_unavailable())?;
        if changed_group.rows_affected() != 1 {
            return Err(stock::conflict());
        }
        let changed_position=sqlx::query("UPDATE hub_stock_positions SET packs=?,revision=? WHERE group_id=? AND location_id=? AND packs=?")
            .bind(i64::from(after_packs.value())).bind(integer(after_revision)?).bind(plan.before.id.as_bytes().as_slice()).bind(plan.before.location.as_bytes().as_slice()).bind(i64::from(plan.before.packs.value()))
            .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if changed_position.rows_affected() != 1 {
            return Err(stock::conflict());
        }
        for allocation in &plan.before.work {
            let changed=sqlx::query("UPDATE hub_group_work SET scoped_packs=? WHERE group_id=? AND work_id=? AND scoped_packs=? AND progress=?")
                .bind(i64::from(after_packs.value())).bind(plan.before.id.as_bytes().as_slice()).bind(allocation.requirement.id.as_bytes().as_slice())
                .bind(i64::from(plan.before.packs.value())).bind(progress(allocation.progress)).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
            if changed.rows_affected() != 1 {
                return Err(stock::conflict());
            }
        }
    }
    if let Some(new) = &surplus {
        insert_surplus_on(tx, &operation, &plan, new).await?;
    }
    handling::write::event_on(
        tx,
        &operation,
        handling::write::EventEffect {
            kind: "count_post",
            prior_events: facts.state.event_count,
            trace: plan.before.trace,
            group: plan.before.id,
            packs: WholePacks::from_value(changed).map_err(|_| corrupt_storage())?,
            from: None,
            to: None,
            occurred: Occurrence::Known(plan.confirmed_on_hand_at),
        },
    )
    .await?;
    if let Some(new) = &surplus {
        handling::write::event_on(
            tx,
            &operation,
            handling::write::EventEffect {
                kind: "count_post",
                prior_events: facts
                    .state
                    .event_count
                    .checked_add(1)
                    .ok_or_else(stock::capacity)?,
                trace: new.population.id,
                group: new.group.id,
                packs: new.group.packs,
                from: None,
                to: Some(&new.population.initial_location),
                occurred: new.population.occurred,
            },
        )
        .await?;
    }
    handling::write::advance_many_on(tx, hub, &facts.state, plan.stock_revision, events).await?;
    let restored = original_on(tx, hub, record).await?;
    if restored.original != plan || restored.surplus != surplus {
        return Err(corrupt_storage());
    }
    // Query the complete current lineage/accounting before the transaction exits.
    handling::lineage::load_on(
        tx,
        hub,
        plan.before.trace,
        &facts.catalog,
        plan.stock_revision,
    )
    .await?;
    if let Some(new) = &surplus {
        handling::lineage::load_on(
            tx,
            hub,
            new.population.id,
            &facts.catalog,
            plan.stock_revision,
        )
        .await?;
    }
    Ok(restored)
}

async fn ensure_surplus_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    new: &CountSurplusPlan,
) -> Result<(), SafeError> {
    let counts=sqlx::query("SELECT (SELECT count(*) FROM hub_receipts) AS populations,(SELECT count(*) FROM hub_stock_groups) AS groups")
        .fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let populations: i64 = counts
        .try_get("populations")
        .map_err(|_| corrupt_storage())?;
    let groups: i64 = counts.try_get("groups").map_err(|_| corrupt_storage())?;
    if populations < 0
        || groups < 0
        || populations > harvestcircle_domain::hub::stock::HUB_TRACE_CAPACITY as i64
        || groups > harvestcircle_domain::hub::movement::HUB_GROUP_CAPACITY as i64
    {
        return Err(corrupt_storage());
    }
    if populations == harvestcircle_domain::hub::stock::HUB_TRACE_CAPACITY as i64
        || groups == harvestcircle_domain::hub::movement::HUB_GROUP_CAPACITY as i64
    {
        return Err(stock::capacity());
    }
    let label = &new
        .group
        .identification
        .as_ref()
        .ok_or_else(corrupt_storage)?
        .label;
    let active:i64=sqlx::query_scalar("SELECT count(*) FROM hub_group_identification i WHERE i.physical_label=? AND EXISTS(SELECT 1 FROM hub_stock_positions p WHERE p.group_id=i.group_id AND p.packs>0)")
        .bind(label.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if active != 0 {
        return Err(stock::conflict());
    }
    Ok(())
}

async fn insert_surplus_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    plan: &CountPostPlan,
    new: &CountSurplusPlan,
) -> Result<(), SafeError> {
    stock::write::insert_population_on(tx, operation, &new.population, new.group.id).await?;
    sqlx::query("INSERT INTO hub_counted_surplus (population_id,reconciliation_id,count_id,related_population_id) VALUES (?,?,?,?)")
        .bind(new.population.id.as_bytes().as_slice()).bind(plan.id.as_bytes().as_slice()).bind(plan.count.id.as_bytes().as_slice()).bind(plan.before.trace.as_bytes().as_slice())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    sqlx::query("INSERT INTO hub_stock_groups (group_id,trace_id,revision) VALUES (?,?,1)")
        .bind(new.group.id.as_bytes().as_slice())
        .bind(new.population.id.as_bytes().as_slice())
        .execute(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    sqlx::query("INSERT INTO hub_stock_positions (group_id,location_id,location_revision,packs,revision) VALUES (?,?,?,?,1)")
        .bind(new.group.id.as_bytes().as_slice()).bind(new.group.location.as_bytes().as_slice()).bind(integer(new.population.initial_location.revision)?)
        .bind(i64::from(new.group.packs.value())).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let identity = new
        .group
        .identification
        .as_ref()
        .ok_or_else(corrupt_storage)?;
    sqlx::query("INSERT INTO hub_group_identification (group_id,parent_group_id,physical_label,observation,created_operation_id) VALUES (?,NULL,?,?,?)")
        .bind(new.group.id.as_bytes().as_slice()).bind(identity.label.as_str()).bind(identity.observation.as_str()).bind(operation.as_str())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    Ok(())
}
