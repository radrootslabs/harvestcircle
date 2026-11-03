//! New intake, originals, fresh optional work and the physical event commit together.
use super::*;
use harvestcircle_domain::hub::movement::{HUB_GROUP_CAPACITY, HUB_WORK_CAPACITY};
use harvestcircle_domain::hub::returns::{return_population_digest, return_result_digest};

impl Database {
    pub async fn apply_hub_return(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: ReturnCommand,
        now: UnixTimestamp,
    ) -> Result<HubReturnReceipt, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move { apply_on(tx, actor, operation, command, now).await })
            })
            .await
            .map_err(map_transaction_error)
    }
    pub async fn hub_return_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubReturnReceipt>, SafeError> {
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
    command: ReturnCommand,
    now: UnixTimestamp,
) -> Result<HubReturnReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let ids = identities(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, ids);
    if let Some(original) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if original.kind != "return" || original.actor != actor || original.hash != hash {
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
    let context = context::load_on(tx, hub.id(), &command).await?;
    let plan = command
        .plan(&context.domain(hub.id()), ids, actor, now)
        .map_err(error)?;
    ensure_capacity_on(tx, &plan).await?;
    let population = plan.population();
    let result_hash = return_result_digest(&hash, &command, &context.domain(hub.id()), &plan)
        .map_err(|_| corrupt_storage())?;
    let population_hash =
        return_population_digest(&hash, hub.id(), plan.catalog_revision, ids, &population)
            .map_err(|_| corrupt_storage())?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "return".into(),
        actor,
        hash,
        catalog_revision: plan.catalog_revision,
        revision: plan.stock_revision,
        result: ids.returned,
        recorded_at: now,
    };
    handling::write::insert_operation_on(tx, hub.id(), &op).await?;
    stock::write::persist_source_on(tx, hub, &operation, &population, ids.source).await?;
    stock::write::insert_population_on(tx, &operation, &population, ids.group).await?;
    let (resolution, reason) = super::resolution(&plan.linkage.resolution);
    sqlx::query("INSERT INTO hub_returns (return_id,operation_id,population_id,policy_version,returned_by_id,returned_by_revision,claimed_departure_id,claimed_group_id,known_departure_id,established_group_id,resolution,unresolved_reason,timing,source_count,intake_work_id,population_hash,result_hash) VALUES (?,?,?,1,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(ids.returned.as_bytes().as_slice()).bind(operation.as_str()).bind(ids.population.as_bytes().as_slice())
        .bind(plan.intake.returned_by.as_ref().map(|r|r.id.as_bytes().as_slice())).bind(plan.intake.returned_by.as_ref().map(|r|integer(r.revision)).transpose()?)
        .bind(command.departure.as_ref().map(|c|c.departure.as_bytes().as_slice())).bind(command.departure.as_ref().and_then(|c|c.group.as_ref()).map(|id|id.as_bytes().as_slice()))
        .bind(context.departure.as_ref().map(|d|d.id.as_bytes().as_slice())).bind(plan.linkage.established_group().map(|id|id.as_bytes().to_vec()))
        .bind(resolution).bind(reason).bind(super::timing(plan.linkage.timing)).bind(i64::try_from(context.totals.len()).map_err(|_|corrupt_storage())?)
        .bind(plan.intake_work.as_ref().map(|w|w.id.as_bytes().as_slice())).bind(population_hash.as_slice()).bind(result_hash.as_slice())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if let Some(departure) = &context.departure {
        let departure_op: String = sqlx::query_scalar(
            "SELECT operation_id FROM hub_departures WHERE departure_id=? LIMIT 1",
        )
        .bind(departure.id.as_bytes().as_slice())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
        let lines = &departure
            .paper
            .versions
            .last()
            .ok_or_else(corrupt_storage)?
            .material
            .lines;
        for (ordinal, line) in lines.iter().enumerate() {
            let total = context
                .totals
                .iter()
                .find(|total| total.original_group == line.group)
                .ok_or_else(corrupt_storage)?;
            sqlx::query("INSERT INTO hub_return_sources (return_id,ordinal,departure_operation_id,original_group_id,previously_returned_packs) VALUES (?,?,?,?,?)")
                .bind(ids.returned.as_bytes().as_slice()).bind(i64::try_from(ordinal).map_err(|_|corrupt_storage())?).bind(&departure_op).bind(line.group.as_bytes().as_slice()).bind(i64::from(total.packs.value())).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
        }
    }
    sqlx::query("INSERT INTO hub_stock_groups (group_id,trace_id,revision) VALUES (?,?,1)")
        .bind(ids.group.as_bytes().as_slice())
        .bind(ids.population.as_bytes().as_slice())
        .execute(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    sqlx::query("INSERT INTO hub_stock_positions (group_id,location_id,location_revision,packs,revision) VALUES (?,?,?,?,1)").bind(ids.group.as_bytes().as_slice()).bind(plan.intake.location.id.as_bytes().as_slice()).bind(integer(plan.intake.location.revision)?).bind(i64::from(command.packs.value())).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    sqlx::query("INSERT INTO hub_group_identification (group_id,parent_group_id,physical_label,observation,created_operation_id) VALUES (?,NULL,?,?,?)")
        .bind(ids.group.as_bytes().as_slice()).bind(command.identification.label.as_str()).bind(command.identification.observation.as_str()).bind(operation.as_str()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if let Some(work) = &plan.intake_work {
        let target = work.target.as_ref().ok_or_else(corrupt_storage)?;
        sqlx::query("INSERT INTO hub_work_requirements (work_id,trace_id,variant_id,target_location_id,target_location_revision,required,completed,no_longer_required,remaining,revision,created_operation_id) VALUES (?,?,?,?,?,?,0,0,?,1,?)")
            .bind(work.id.as_bytes().as_slice()).bind(ids.population.as_bytes().as_slice()).bind(command.variant.as_bytes().as_slice()).bind(target.id.as_bytes().as_slice()).bind(integer(target.revision)?)
            .bind(i64::from(command.packs.value())).bind(i64::from(command.packs.value())).bind(operation.as_str()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
        sqlx::query("INSERT INTO hub_group_work (group_id,work_id,progress,scoped_packs) VALUES (?,?,'pending',?)")
            .bind(ids.group.as_bytes().as_slice()).bind(work.id.as_bytes().as_slice()).bind(i64::from(command.packs.value())).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    handling::write::event_on(
        tx,
        &operation,
        handling::write::EventEffect {
            kind: "return",
            prior_events: context.state.event_count,
            trace: ids.population,
            group: ids.group,
            packs: command.packs,
            from: None,
            to: Some(&population.initial_location),
            occurred: command.occurred,
        },
    )
    .await?;
    handling::write::advance_many_on(tx, hub.id(), &context.state, plan.stock_revision, 1).await?;
    let restored = read::original_on(tx, hub.id(), op).await?;
    if restored.original != plan
        || handling::read::group_on(tx, hub.id(), ids.group, &context.catalog).await? != plan.group
    {
        return Err(corrupt_storage());
    }
    Ok(restored)
}
async fn ensure_capacity_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    plan: &ReturnPlan,
) -> Result<(), SafeError> {
    let counts=sqlx::query("SELECT (SELECT count(*) FROM hub_receipts) AS populations,(SELECT count(*) FROM hub_stock_groups) AS groups,(SELECT count(*) FROM hub_work_requirements) AS work").fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    for (field, limit, growing) in [
        (
            "populations",
            harvestcircle_domain::hub::stock::HUB_TRACE_CAPACITY,
            true,
        ),
        ("groups", HUB_GROUP_CAPACITY, true),
        ("work", HUB_WORK_CAPACITY, plan.intake_work.is_some()),
    ] {
        let count: i64 = counts.try_get(field).map_err(|_| corrupt_storage())?;
        let limit = i64::try_from(limit).map_err(|_| corrupt_storage())?;
        if !(0..=limit).contains(&count) {
            return Err(corrupt_storage());
        }
        if growing && count == limit {
            return Err(stock::capacity());
        }
    }
    let labels:i64=sqlx::query_scalar("SELECT count(*) FROM hub_group_identification i WHERE i.physical_label=? AND EXISTS(SELECT 1 FROM hub_stock_positions p WHERE p.group_id=i.group_id AND p.packs>0)")
        .bind(plan.group.identification.as_ref().ok_or_else(corrupt_storage)?.label.as_str()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if labels != 0 {
        return Err(stock::conflict());
    }
    Ok(())
}
fn integer(revision: HubRevision) -> Result<i64, SafeError> {
    i64::try_from(revision.value()).map_err(|_| corrupt_storage())
}
