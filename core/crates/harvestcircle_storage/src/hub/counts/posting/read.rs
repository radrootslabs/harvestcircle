//! Originals use captured counters and immutable references/identity, never current balances.
use super::*;

struct StoredWork {
    allocation: WorkAllocation,
    after: WorkRequirement,
    scoped: WholePacks,
    on_hand: WholePacks,
    removed: WholePacks,
    removed_progress: Option<WorkProgress>,
}

pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: stock::read::OperationRecord,
) -> Result<HubCountPostReceipt, SafeError> {
    if operation.kind != "count_post" {
        return Err(stock::conflict());
    }
    let row = sqlx::query(concat!(
        "SELECT substr(reconciliation_id,1,17) AS reconciliation_id,substr(count_id,1,17) AS count_id,policy_version,substr(effect_kind,1,16) AS effect_kind,changed_packs,after_on_hand_packs,after_group_revision,",
        "before_causal_floor_unix_s,confirmed_on_hand_at_unix_s,scope_work_count,changed_work_count,substr(surplus_population_id,1,17) AS surplus_population_id,",
        "substr(CAST(reason AS BLOB),1,513) AS reason,substr(result_hash,1,33) AS result_hash FROM hub_count_reconciliations WHERE operation_id=? LIMIT 1"
    )).bind(operation.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if row
        .try_get::<i64, _>("policy_version")
        .map_err(|_| corrupt_storage())?
        != 1
    {
        return Err(corrupt_storage());
    }
    let count = super::super::read::by_id_on(tx, hub, record_id(&row, "count_id")?)
        .await?
        .original;
    let scope = &count.scope;
    let identity = sqlx::query(concat!(
        "SELECT substr(g.trace_id,1,17) AS trace_id,substr(i.parent_group_id,1,17) AS parent,substr(CAST(i.physical_label AS BLOB),1,121) AS label,",
        "substr(CAST(i.observation AS BLOB),1,513) AS observation FROM hub_group_identification i JOIN hub_stock_groups g ON g.group_id=i.group_id WHERE i.group_id=? LIMIT 1"
    )).bind(scope.group.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if record_id(&identity, "trace_id")? != scope.trace {
        return Err(corrupt_storage());
    }
    let before_identification = identification(&identity, "label", "observation")?;
    let scope_count: usize = usize::try_from(
        row.try_get::<i64, _>("scope_work_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if scope_count > harvestcircle_domain::hub::movement::HUB_GROUP_REQUIREMENTS {
        return Err(corrupt_storage());
    }
    let works = work_on(tx, hub, record_id(&row, "reconciliation_id")?, scope_count).await?;
    let causal: Option<i64> = row
        .try_get("before_causal_floor_unix_s")
        .map_err(|_| corrupt_storage())?;
    let before = PhysicalStockGroup {
        id: scope.group,
        trace: scope.trace,
        variant: scope.variant.id,
        parent: stock::read::optional_id(&identity, "parent")?,
        identification: before_identification,
        location: scope.location.id,
        packs: scope.recorded_packs,
        revision: scope.group_revision,
        causal_floor: causal.map(stock::read::timestamp).transpose()?,
        work: works.iter().map(|w| w.allocation.clone()).collect(),
    };
    let references = CountPostReferences {
        product: reference::record_at_catalog_on(
            tx,
            hub,
            scope.product.id,
            operation.catalog_revision,
        )
        .await?,
        variant: reference::record_at_catalog_on(
            tx,
            hub,
            scope.variant.id,
            operation.catalog_revision,
        )
        .await?,
        location: reference::record_at_catalog_on(
            tx,
            hub,
            scope.location.id,
            operation.catalog_revision,
        )
        .await?,
    };
    let changed = whole_packs(
        row.try_get("changed_packs")
            .map_err(|_| corrupt_storage())?,
    )?;
    let after_packs = whole_packs(
        row.try_get("after_on_hand_packs")
            .map_err(|_| corrupt_storage())?,
    )?;
    let after_revision = revision_field(&row, "after_group_revision")?;
    let tag: String = row.try_get("effect_kind").map_err(|_| corrupt_storage())?;
    let ids = CountSurplusIdentities::from_operation(operation_id(&operation.id)?)
        .map_err(|_| corrupt_storage())?;
    let effect = match tag.as_str() {
        "none" if changed.value() == 0 => CountEffect::NoVariance,
        "shortage" => CountEffect::Shortage {
            packs: changed,
            after_packs,
            after_revision,
            work_changes: works
                .iter()
                .filter(|w| w.allocation.progress == WorkProgress::Pending && w.removed.value() > 0)
                .map(|w| CountWorkChange {
                    before: w.allocation.requirement.clone(),
                    after: w.after.clone(),
                    retired: w.removed,
                })
                .collect(),
        },
        "surplus" => {
            let identity = sqlx::query("SELECT substr(CAST(physical_label AS BLOB),1,121) AS label,substr(CAST(observation AS BLOB),1,513) AS observation FROM hub_group_identification WHERE group_id=? AND parent_group_id IS NULL AND created_operation_id=? LIMIT 1")
                .bind(ids.group.as_bytes().as_slice()).bind(operation.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
            CountEffect::Surplus {
                packs: changed,
                identification: identification(&identity, "label", "observation")?
                    .ok_or_else(corrupt_storage)?,
                source: SourceAttribution::Unknown,
                acceptance: AcceptanceStatus::ReviewRequired,
            }
        }
        _ => return Err(corrupt_storage()),
    };
    let plan = CountPostPlan {
        id: record_id(&row, "reconciliation_id")?,
        operation: operation_id(&operation.id)?,
        count,
        before,
        stock_revision: operation.revision,
        catalog_revision: operation.catalog_revision,
        references,
        actor: operation.actor,
        recorded_at: operation.recorded_at,
        confirmed_on_hand_at: stock::read::timestamp(
            row.try_get("confirmed_on_hand_at_unix_s")
                .map_err(|_| corrupt_storage())?,
        )?,
        effect,
        reason: SourceProvenance::new(stock::read::text(&row, "reason")?)
            .map_err(|_| corrupt_storage())?,
    };
    plan.validate().map_err(|_| corrupt_storage())?;
    let (expected_after, expected_revision, removed) = match &plan.effect {
        CountEffect::Shortage {
            packs,
            after_packs,
            after_revision,
            ..
        } => (*after_packs, *after_revision, *packs),
        _ => (
            plan.before.packs,
            plan.before.revision,
            WholePacks::from_value(0).map_err(|_| corrupt_storage())?,
        ),
    };
    if plan.id != operation.result
        || expected_after != after_packs
        || expected_revision != after_revision
    {
        return Err(corrupt_storage());
    }
    let mut changed_work = 0;
    for work in &works {
        let expected_progress = if removed.value() == 0 {
            None
        } else {
            Some(match work.allocation.progress {
                WorkProgress::Pending => WorkProgress::NoLongerRequired,
                p => p,
            })
        };
        let expected_after = match &plan.effect {
            CountEffect::Shortage { work_changes, .. } => work_changes
                .iter()
                .find(|c| c.before.id == work.allocation.requirement.id)
                .map(|c| &c.after)
                .unwrap_or(&work.allocation.requirement),
            _ => &work.allocation.requirement,
        };
        if work.scoped != plan.before.packs
            || work.on_hand != after_packs
            || work.removed != removed
            || work.removed_progress != expected_progress
            || work.after != *expected_after
        {
            return Err(corrupt_storage());
        }
        if work.after != work.allocation.requirement {
            changed_work += 1;
        }
    }
    if row
        .try_get::<i64, _>("changed_work_count")
        .map_err(|_| corrupt_storage())?
        != changed_work
    {
        return Err(corrupt_storage());
    }
    let digest: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    if digest.as_slice()
        != count_post_result_digest(&operation.hash, &plan).map_err(|_| corrupt_storage())?
    {
        return Err(corrupt_storage());
    }
    let command = CountPostCommand {
        expected_stock_revision: HubRevision::from_value(operation.revision.value() - 1)
            .map_err(|_| corrupt_storage())?,
        expected_catalog_revision: operation.catalog_revision,
        count: plan.count.id,
        reviewed_activity: true,
        surplus_identification: match &plan.effect {
            CountEffect::Surplus { identification, .. } => Some(identification.clone()),
            _ => None,
        },
        reason: plan.reason.clone(),
    };
    if command.fingerprint(hub, operation.actor, plan.operation) != operation.hash {
        return Err(corrupt_storage());
    }
    let surplus = plan.surplus().map_err(|_| corrupt_storage())?;
    match (
        &surplus,
        stock::read::optional_id(&row, "surplus_population_id")?,
    ) {
        (None, None) => (),
        (Some(new), Some(id)) if id == new.population.id => {
            verify_surplus_on(tx, hub, &operation, &plan, new).await?
        }
        _ => return Err(corrupt_storage()),
    }
    verify_events_on(tx, &operation, &plan, surplus.as_ref()).await?;
    Ok(HubCountPostReceipt {
        operation: operation.id,
        original: plan,
        surplus,
    })
}

fn identification(
    row: &sqlx::sqlite::SqliteRow,
    label: &str,
    note: &str,
) -> Result<Option<PhysicalIdentification>, SafeError> {
    let label: Option<Vec<u8>> = row.try_get(label).map_err(|_| corrupt_storage())?;
    let note: Option<Vec<u8>> = row.try_get(note).map_err(|_| corrupt_storage())?;
    match (label, note) {
        (None, None) => Ok(None),
        (Some(label), Some(note)) => Ok(Some(PhysicalIdentification {
            label: HubLabel::new(String::from_utf8(label).map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?,
            observation: SourceProvenance::new(
                String::from_utf8(note).map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?,
        })),
        _ => Err(corrupt_storage()),
    }
}

async fn work_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
    count: usize,
) -> Result<Vec<StoredWork>, SafeError> {
    let rows=sqlx::query(concat!("SELECT ordinal,substr(work_id,1,17) AS work_id,substr(before_progress,1,33) AS before_progress,before_scoped_packs,on_hand_after,removed_packs,substr(removed_progress,1,33) AS removed_progress,required,",
        "before_completed,before_no_longer_required,before_remaining,before_revision,after_completed,after_no_longer_required,after_remaining,after_revision FROM hub_count_reconciliation_work WHERE reconciliation_id=? ORDER BY ordinal LIMIT 65"))
        .bind(id.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() != count {
        return Err(corrupt_storage());
    }
    let mut result = Vec::with_capacity(count);
    for (ordinal, row) in rows.into_iter().enumerate() {
        if row
            .try_get::<i64, _>("ordinal")
            .map_err(|_| corrupt_storage())?
            != ordinal as i64
        {
            return Err(corrupt_storage());
        }
        let definition =
            handling::read::definition_on(tx, hub, record_id(&row, "work_id")?).await?;
        if definition.required
            != whole_packs(row.try_get("required").map_err(|_| corrupt_storage())?)?
        {
            return Err(corrupt_storage());
        }
        let quantity = |a: &str, b: &str, c: &str| {
            QuantityWork::restore(
                definition.required,
                whole_packs(row.try_get(a).map_err(|_| corrupt_storage())?)?,
                whole_packs(row.try_get(b).map_err(|_| corrupt_storage())?)?,
                whole_packs(row.try_get(c).map_err(|_| corrupt_storage())?)?,
            )
            .map_err(|_| corrupt_storage())
        };
        let before = definition.at(
            quantity(
                "before_completed",
                "before_no_longer_required",
                "before_remaining",
            )?,
            revision_field(&row, "before_revision")?,
        )?;
        let after = definition.at(
            quantity(
                "after_completed",
                "after_no_longer_required",
                "after_remaining",
            )?,
            revision_field(&row, "after_revision")?,
        )?;
        result.push(StoredWork {
            allocation: WorkAllocation {
                requirement: before,
                progress: decode_progress(
                    &row.try_get::<String, _>("before_progress")
                        .map_err(|_| corrupt_storage())?,
                )?,
            },
            after,
            scoped: whole_packs(
                row.try_get("before_scoped_packs")
                    .map_err(|_| corrupt_storage())?,
            )?,
            on_hand: whole_packs(
                row.try_get("on_hand_after")
                    .map_err(|_| corrupt_storage())?,
            )?,
            removed: whole_packs(
                row.try_get("removed_packs")
                    .map_err(|_| corrupt_storage())?,
            )?,
            removed_progress: row
                .try_get::<Option<String>, _>("removed_progress")
                .map_err(|_| corrupt_storage())?
                .map(|s| decode_progress(&s))
                .transpose()?,
        });
    }
    Ok(result)
}

async fn verify_surplus_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: &stock::read::OperationRecord,
    plan: &CountPostPlan,
    new: &CountSurplusPlan,
) -> Result<(), SafeError> {
    let fields =
        stock::read::population_fields_for_on(tx, hub, operation, new.population.id).await?;
    if fields.origin_kind != "surplus"
        || fields.trace != new.population.id
        || fields.group != new.group.id
        || fields.supplier.is_some()
        || fields.producer.is_some()
        || fields.planned_location.is_some()
        || fields.source != SourceAttribution::Unknown
        || fields.packs != new.population.original_packs
        || fields.occurred != new.population.occurred
        || fields.observation != new.population.observation
    {
        return Err(corrupt_storage());
    }
    for record in [
        &new.population.product,
        &new.population.variant,
        &new.population.initial_location,
    ] {
        if fields
            .catalog
            .get(record.id)
            .map_err(|_| corrupt_storage())?
            != record
        {
            return Err(corrupt_storage());
        }
    }
    let valid:i64=sqlx::query_scalar("SELECT count(*) FROM hub_counted_surplus WHERE population_id=? AND reconciliation_id=? AND count_id=? AND related_population_id=?")
        .bind(new.population.id.as_bytes().as_slice()).bind(plan.id.as_bytes().as_slice()).bind(plan.count.id.as_bytes().as_slice()).bind(plan.before.trace.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if valid != 1 {
        return Err(corrupt_storage());
    }
    Ok(())
}

async fn verify_events_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &stock::read::OperationRecord,
    plan: &CountPostPlan,
    new: Option<&CountSurplusPlan>,
) -> Result<(), SafeError> {
    let packs = match plan.effect {
        CountEffect::NoVariance => 0,
        CountEffect::Shortage { packs, .. } | CountEffect::Surplus { packs, .. } => packs.value(),
    };
    let rows=sqlx::query("SELECT substr(trace_id,1,17) AS trace_id,substr(group_id,1,17) AS group_id,packs,substr(from_location_id,1,17) AS from_location_id,substr(to_location_id,1,17) AS to_location_id,from_location_revision,to_location_revision,substr(occurrence_kind,1,9) AS occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s FROM hub_stock_events WHERE operation_id=? AND kind='count_post' ORDER BY sequence LIMIT 3")
        .bind(operation.id.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() != 1 + usize::from(new.is_some()) {
        return Err(corrupt_storage());
    }
    for (index, row) in rows.into_iter().enumerate() {
        let (trace, group, to) = if index == 0 {
            (plan.before.trace, plan.before.id, None)
        } else {
            let new = new.ok_or_else(corrupt_storage)?;
            (
                new.population.id,
                new.group.id,
                Some(&new.population.initial_location),
            )
        };
        if record_id(&row, "trace_id")? != trace
            || record_id(&row, "group_id")? != group
            || whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?.value() != packs
            || stock::read::optional_id(&row, "from_location_id")?.is_some()
            || row
                .try_get::<Option<i64>, _>("from_location_revision")
                .map_err(|_| corrupt_storage())?
                .is_some()
            || stock::read::optional_id(&row, "to_location_id")? != to.map(|r| r.id)
            || row
                .try_get::<Option<i64>, _>("to_location_revision")
                .map_err(|_| corrupt_storage())?
                != to.map(|r| integer(r.revision)).transpose()?
            || stock::read::occurrence(&row)? != Occurrence::Known(plan.confirmed_on_hand_at)
        {
            return Err(corrupt_storage());
        }
    }
    Ok(())
}

pub(in crate::hub) async fn losses_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
) -> Result<Vec<TraceCountLoss>, SafeError> {
    let operations:Vec<String>=sqlx::query_scalar(concat!("SELECT substr(r.operation_id,1,37) FROM hub_count_reconciliations r JOIN hub_count_observations c ON c.count_id=r.count_id JOIN hub_stock_operations o ON o.operation_id=r.operation_id ",
        "WHERE c.trace_id=? AND r.effect_kind='shortage' AND o.hub_id=? ORDER BY o.stock_revision LIMIT 4097"))
        .bind(trace.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if operations.len() > HUB_COUNT_CAPACITY {
        return Err(corrupt_storage());
    }
    let mut result = Vec::with_capacity(operations.len());
    for id in operations {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let original = original_on(tx, hub, op).await?.original;
        let CountEffect::Shortage { packs, .. } = original.effect else {
            return Err(corrupt_storage());
        };
        result.push(TraceCountLoss {
            reconciliation: original.id,
            operation: original.operation,
            count: original.count.id,
            population: original.before.trace,
            group: original.before.id,
            stock_revision: original.stock_revision,
            packs,
            confirmed_on_hand_at: original.confirmed_on_hand_at,
            recorded_at: original.recorded_at,
            actor: original.actor,
            reason: original.reason,
        });
    }
    Ok(result)
}
