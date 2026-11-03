//! Immutable observed removal readback does not reconstruct current effect permission.
use super::*;
use harvestcircle_domain::hub::departure::{ConsumedStockGroup, DepartureWorkChange};
use harvestcircle_domain::hub::movement::{
    PhysicalIdentification, PhysicalStockGroup, WorkAllocation, WorkProgress,
};
use harvestcircle_domain::hub::reference::{HubLabel, ReferenceValue, SourceProvenance};
use harvestcircle_domain::hub::stock::QuantityWork;
use std::collections::BTreeMap;

pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubDispositionReceipt, SafeError> {
    if op.kind != "disposition" {
        return Err(stock::conflict());
    }
    let ids = identities(&op.id).map_err(|_| corrupt_storage())?;
    let row=sqlx::query("SELECT substr(disposition_id,1,17) AS disposition_id,policy_version,substr(group_id,1,17) AS group_id,substr(trace_id,1,17) AS trace_id,before_group_revision,after_group_revision,packs,substr(location_id,1,17) AS location_id,location_revision,before_causal_floor_unix_s,after_causal_floor_unix_s,scope_count,work_change_count,substr(disposition_kind,1,33) AS disposition_kind,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,substr(CAST(observation AS BLOB),1,513) AS observation,substr(result_hash,1,33) AS result_hash FROM hub_dispositions WHERE operation_id=? LIMIT 1").bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if op.result != ids.disposition
        || record_id(&row, "disposition_id")? != ids.disposition
        || row
            .try_get::<i64, _>("policy_version")
            .map_err(|_| corrupt_storage())?
            != 1
    {
        return Err(corrupt_storage());
    }
    let group = record_id(&row, "group_id")?;
    let trace = record_id(&row, "trace_id")?;
    let population = handling::read::trace_on(tx, hub, trace).await?;
    let variant =
        reference::record_at_catalog_on(tx, hub, population.variant.id, op.catalog_revision)
            .await?;
    let ReferenceValue::Pack { product, .. } = variant.value else {
        return Err(corrupt_storage());
    };
    let product = reference::record_at_catalog_on(tx, hub, product, op.catalog_revision).await?;
    let location = reference::record_at_catalog_on(
        tx,
        hub,
        record_id(&row, "location_id")?,
        op.catalog_revision,
    )
    .await?;
    if location.revision != revision_field(&row, "location_revision")? {
        return Err(corrupt_storage());
    }
    let birth=sqlx::query("SELECT substr(g.trace_id,1,17) AS trace_id,substr(i.parent_group_id,1,17) AS parent_id,substr(CAST(i.physical_label AS BLOB),1,121) AS physical_label,substr(CAST(i.observation AS BLOB),1,513) AS physical_observation FROM hub_stock_groups g JOIN hub_group_identification i ON i.group_id=g.group_id WHERE g.group_id=? LIMIT 1")
        .bind(group.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if record_id(&birth, "trace_id")? != trace {
        return Err(corrupt_storage());
    }
    let label: Option<Vec<u8>> = birth
        .try_get("physical_label")
        .map_err(|_| corrupt_storage())?;
    let observation: Option<Vec<u8>> = birth
        .try_get("physical_observation")
        .map_err(|_| corrupt_storage())?;
    let identification = match (label, observation) {
        (None, None) => None,
        (Some(label), Some(observation)) => Some(PhysicalIdentification {
            label: HubLabel::new(String::from_utf8(label).map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?,
            observation: SourceProvenance::new(
                String::from_utf8(observation).map_err(|_| corrupt_storage())?,
            )
            .map_err(|_| corrupt_storage())?,
        }),
        _ => return Err(corrupt_storage()),
    };
    let scope_count = usize::try_from(
        row.try_get::<i64, _>("scope_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let change_count = usize::try_from(
        row.try_get::<i64, _>("work_change_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if scope_count > 64 || change_count > scope_count {
        return Err(corrupt_storage());
    }
    let scopes=sqlx::query("SELECT substr(work_id,1,17) AS work_id,substr(before_progress,1,33) AS before_progress,before_work_revision,before_completed,before_no_longer_required,before_remaining FROM hub_disposition_work_scope WHERE operation_id=? ORDER BY work_id LIMIT 65")
        .bind(op.id.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if scopes.len() != scope_count {
        return Err(corrupt_storage());
    }
    let mut work = Vec::with_capacity(scopes.len());
    let mut before_work = BTreeMap::new();
    for scope in scopes {
        let id = record_id(&scope, "work_id")?;
        let definition = handling::read::definition_on(tx, hub, id).await?;
        if definition.trace != trace || definition.variant != population.variant.id {
            return Err(corrupt_storage());
        }
        let quantity = QuantityWork::restore(
            definition.required,
            whole_packs(
                scope
                    .try_get("before_completed")
                    .map_err(|_| corrupt_storage())?,
            )?,
            whole_packs(
                scope
                    .try_get("before_no_longer_required")
                    .map_err(|_| corrupt_storage())?,
            )?,
            whole_packs(
                scope
                    .try_get("before_remaining")
                    .map_err(|_| corrupt_storage())?,
            )?,
        )
        .map_err(|_| corrupt_storage())?;
        let requirement =
            definition.at(quantity, revision_field(&scope, "before_work_revision")?)?;
        let progress = match scope
            .try_get::<String, _>("before_progress")
            .map_err(|_| corrupt_storage())?
            .as_str()
        {
            "pending" => WorkProgress::Pending,
            "completed" => WorkProgress::Completed,
            "no_longer_required" => WorkProgress::NoLongerRequired,
            _ => return Err(corrupt_storage()),
        };
        if before_work.insert(id, requirement.clone()).is_some() {
            return Err(corrupt_storage());
        }
        work.push(WorkAllocation {
            requirement,
            progress,
        });
    }
    let changes=sqlx::query("SELECT substr(work_id,1,17) AS work_id,before_completed,before_no_longer_required,before_remaining,before_revision,after_completed,after_no_longer_required,after_remaining,after_revision FROM hub_work_changes WHERE operation_id=? ORDER BY work_id LIMIT 65")
        .bind(op.id.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if changes.len() != change_count {
        return Err(corrupt_storage());
    }
    let packs = whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?;
    let mut work_changes = Vec::with_capacity(changes.len());
    for change in changes {
        let before = before_work
            .get(&record_id(&change, "work_id")?)
            .ok_or_else(corrupt_storage)?
            .clone();
        if before.quantity.completed()
            != whole_packs(
                change
                    .try_get("before_completed")
                    .map_err(|_| corrupt_storage())?,
            )?
            || before.quantity.no_longer_required()
                != whole_packs(
                    change
                        .try_get("before_no_longer_required")
                        .map_err(|_| corrupt_storage())?,
                )?
            || before.quantity.remaining()
                != whole_packs(
                    change
                        .try_get("before_remaining")
                        .map_err(|_| corrupt_storage())?,
                )?
            || before.revision != revision_field(&change, "before_revision")?
        {
            return Err(corrupt_storage());
        }
        let mut after = before.clone();
        after.quantity = QuantityWork::restore(
            before.quantity.required(),
            whole_packs(
                change
                    .try_get("after_completed")
                    .map_err(|_| corrupt_storage())?,
            )?,
            whole_packs(
                change
                    .try_get("after_no_longer_required")
                    .map_err(|_| corrupt_storage())?,
            )?,
            whole_packs(
                change
                    .try_get("after_remaining")
                    .map_err(|_| corrupt_storage())?,
            )?,
        )
        .map_err(|_| corrupt_storage())?;
        after.revision = revision_field(&change, "after_revision")?;
        work_changes.push(DepartureWorkChange {
            before,
            after,
            retired: packs,
        });
    }
    let before_floor = row
        .try_get::<Option<i64>, _>("before_causal_floor_unix_s")
        .map_err(|_| corrupt_storage())?
        .map(stock::read::timestamp)
        .transpose()?;
    let after_floor = row
        .try_get::<Option<i64>, _>("after_causal_floor_unix_s")
        .map_err(|_| corrupt_storage())?
        .map(stock::read::timestamp)
        .transpose()?;
    let kind = match row
        .try_get::<String, _>("disposition_kind")
        .map_err(|_| corrupt_storage())?
        .as_str()
    {
        "destroyed" => DispositionKind::Destroyed,
        "other_observed_removal" => DispositionKind::OtherObservedRemoval,
        _ => return Err(corrupt_storage()),
    };
    let original = DispositionRecord {
        identities: ids,
        hub,
        stock_revision: op.revision,
        catalog_revision: op.catalog_revision,
        product,
        variant,
        location: location.clone(),
        kind,
        actor: op.actor,
        recorded_at: op.recorded_at,
        occurred: stock::read::occurrence(&row)?,
        observation: SourceProvenance::new(stock::read::text(&row, "observation")?)
            .map_err(|_| corrupt_storage())?,
        consumed: ConsumedStockGroup {
            before: PhysicalStockGroup {
                id: group,
                trace,
                variant: population.variant.id,
                parent: stock::read::optional_id(&birth, "parent_id")?,
                identification,
                location: location.id,
                packs,
                revision: revision_field(&row, "before_group_revision")?,
                causal_floor: before_floor,
                work,
            },
            after_revision: revision_field(&row, "after_group_revision")?,
            causal_floor: after_floor,
        },
        work_changes,
    };
    let result: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    if disposition_result_digest(&op.hash, &original)
        .map_err(|_| corrupt_storage())?
        .as_slice()
        != result
    {
        return Err(corrupt_storage());
    }
    Ok(HubDispositionReceipt {
        operation: op.id,
        original,
    })
}

pub(in crate::hub) async fn require_unconsumed_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    group: HubRecordId,
) -> Result<(), SafeError> {
    let op: Option<String> = sqlx::query_scalar(
        "SELECT substr(operation_id,1,37) FROM hub_dispositions WHERE group_id=? LIMIT 1",
    )
    .bind(group.as_bytes().as_slice())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let Some(op) = op else {
        return Ok(());
    };
    let op = DurableRequestId::parse(op).map_err(|_| corrupt_storage())?;
    let record = stock::read::operation_on(tx, hub, &op)
        .await?
        .ok_or_else(corrupt_storage)?;
    let original = original_on(tx, hub, record).await?.original;
    let current: i64 = sqlx::query_scalar("SELECT revision FROM hub_stock_groups WHERE group_id=?")
        .bind(group.as_bytes().as_slice())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    let positive: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_stock_positions WHERE group_id=? AND packs>0")
            .bind(group.as_bytes().as_slice())
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
    if stock::read::revision(current)? != original.consumed.after_revision || positive != 0 {
        return Err(corrupt_storage());
    }
    Err(harvestcircle_domain::SafeError::new(
        harvestcircle_domain::SafeErrorCode::HubConflict,
        harvestcircle_domain::SafeMessage::new(
            "This stock was already removed. Review its retained disposition and physical history.",
        ),
    ))
}
