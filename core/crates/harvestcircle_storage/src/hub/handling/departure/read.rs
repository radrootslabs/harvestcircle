//! Original departure reads use immutable paper/line/work snapshots, not balances.
use super::*;
use crate::Database;
use harvestcircle_domain::hub::departure::{ConsumedStockGroup, departure_result_digest};
use harvestcircle_domain::hub::movement::{PhysicalStockGroup, WorkAllocation, WorkProgress};
use harvestcircle_domain::hub::reference::SourceProvenance;
use harvestcircle_domain::hub::stock::QuantityWork;
use sqlx::Row;
use stock::read::{OperationRecord, record_id, revision_field, whole_packs};

/// A verified exit is a changed operating context, not a corrupt missing position.
/// All current-group consumers share this check, including stale handling commands.
pub(in crate::hub) async fn require_on_hand_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    group: HubRecordId,
    catalog: &harvestcircle_domain::hub::reference::ReferenceCatalog,
) -> Result<(), SafeError> {
    let operation: Option<String> = sqlx::query_scalar(
        "SELECT substr(operation_id,1,37) FROM hub_departure_lines WHERE group_id=? LIMIT 1",
    )
    .bind(group.as_bytes().as_slice())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let Some(operation) = operation else {
        return Ok(());
    };
    let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
    let operation = stock::read::operation_on(tx, hub, &operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    let original = original_on(tx, hub, operation).await?;
    let consumed = original
        .record
        .consumed
        .iter()
        .find(|effect| effect.before.id == group)
        .ok_or_else(corrupt_storage)?;
    let state = stock::read::state_on(tx, hub).await?;
    let current =
        super::super::lineage::load_on(tx, hub, consumed.before.trace, catalog, state.revision)
            .await?;
    let current = current
        .rows
        .iter()
        .find(|row| row.id == group)
        .ok_or_else(corrupt_storage)?;
    if current.packs.value() != 0 || current.revision != consumed.after_revision {
        return Err(corrupt_storage());
    }
    Err(SafeError::new(
        SafeErrorCode::HubConflict,
        SafeMessage::new(
            "This identified stock has already departed. Review its retained lineage and departure history.",
        ),
    ))
}

pub(super) struct Header {
    id: HubRecordId,
    preparation: HubRecordId,
    preparation_revision: HubRevision,
    identification: HubRecordId,
    lines: usize,
    changes: usize,
    occurred: Occurrence,
    observation: SourceProvenance,
    digest: [u8; 32],
}
async fn header_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &OperationRecord,
) -> Result<Header, SafeError> {
    let row=sqlx::query(concat!(
        "SELECT substr(departure_id,1,17) AS departure_id,substr(preparation_id,1,17) AS preparation_id,preparation_revision,substr(identification_id,1,17) AS identification_id,",
        "line_count,work_change_count,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,substr(CAST(observation AS BLOB),1,513) AS observation,substr(result_hash,1,33) AS result_hash FROM hub_departures WHERE operation_id=? LIMIT 1"
    )).bind(operation.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let id = record_id(&row, "departure_id")?;
    let lines = usize::try_from(
        row.try_get::<i64, _>("line_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let changes = usize::try_from(
        row.try_get::<i64, _>("work_change_count")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    if operation.kind != "departure"
        || operation.result != id
        || identities(&operation.id)?.departure != id
        || !(1..=64).contains(&lines)
        || changes > harvestcircle_domain::hub::departure::HUB_DEPARTURE_WORK_CHANGES
    {
        return Err(corrupt_storage());
    }
    let bytes: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    Ok(Header {
        id,
        preparation: record_id(&row, "preparation_id")?,
        preparation_revision: revision_field(&row, "preparation_revision")?,
        identification: record_id(&row, "identification_id")?,
        lines,
        changes,
        occurred: stock::read::occurrence(&row)?,
        observation: SourceProvenance::new(stock::read::text(&row, "observation")?)
            .map_err(|_| corrupt_storage())?,
        digest: bytes.try_into().map_err(|_| corrupt_storage())?,
    })
}
pub(in crate::hub) async fn usage_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    preparation: HubRecordId,
) -> Result<Option<HubDepartureUse>, SafeError> {
    let operation: Option<String> = sqlx::query_scalar(
        "SELECT substr(operation_id,1,37) FROM hub_departures WHERE preparation_id=? LIMIT 1",
    )
    .bind(preparation.as_bytes().as_slice())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let Some(operation) = operation else {
        return Ok(None);
    };
    let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
    let original = stock::read::operation_on(tx, hub, &operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    let header = header_on(tx, &original).await?;
    if header.preparation != preparation {
        return Err(corrupt_storage());
    }
    Ok(Some(HubDepartureUse {
        id: header.id,
        operation,
        preparation,
        identification: header.identification,
        revision: original.revision,
        actor: original.actor,
        recorded_at: original.recorded_at,
    }))
}
fn clock(value: Option<i64>) -> Result<Option<UnixTimestamp>, SafeError> {
    value
        .map(|time| UnixTimestamp::from_seconds(time).ok_or_else(corrupt_storage))
        .transpose()
}
pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: OperationRecord,
) -> Result<HubDepartureReceipt, SafeError> {
    let header = header_on(tx, &operation).await?;
    let paper = super::super::dispatch::read::record_revision_on(
        tx,
        hub,
        header.preparation,
        header.preparation_revision,
    )
    .await?;
    let issued = paper.versions.last().ok_or_else(corrupt_storage)?;
    if issued.id != header.identification || issued.material.lines.len() != header.lines {
        return Err(corrupt_storage());
    }
    let rows=sqlx::query(concat!(
        "SELECT l.ordinal,substr(l.group_id,1,17) AS group_id,substr(l.trace_id,1,17) AS trace_id,l.before_group_revision,l.after_group_revision,l.packs,",
        "substr(l.location_id,1,17) AS location_id,l.location_revision,l.before_causal_floor_unix_s,l.after_causal_floor_unix_s,l.scope_count,substr(g.parent_group_id,1,17) AS parent_group_id ",
        "FROM hub_departure_lines l JOIN hub_group_identification g ON g.group_id=l.group_id WHERE l.operation_id=? ORDER BY l.ordinal LIMIT 65"
    )).bind(operation.id.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() != header.lines {
        return Err(corrupt_storage());
    }
    let mut consumed = Vec::with_capacity(rows.len());
    for (ordinal, (row, line)) in rows.into_iter().zip(&issued.material.lines).enumerate() {
        if row
            .try_get::<i64, _>("ordinal")
            .map_err(|_| corrupt_storage())?
            != ordinal as i64
            || record_id(&row, "group_id")? != line.group
            || record_id(&row, "trace_id")? != line.trace.id
            || record_id(&row, "location_id")? != line.location.id
            || revision_field(&row, "location_revision")? != line.location.revision
        {
            return Err(corrupt_storage());
        }
        let count = usize::try_from(
            row.try_get::<i64, _>("scope_count")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        if count > harvestcircle_domain::hub::movement::HUB_GROUP_REQUIREMENTS {
            return Err(corrupt_storage());
        }
        let scopes=sqlx::query(concat!(
            "SELECT substr(work_id,1,17) AS work_id,substr(before_progress,1,19) AS before_progress,before_work_revision,before_completed,before_no_longer_required,before_remaining ",
            "FROM hub_departure_work_scope WHERE operation_id=? AND line_ordinal=? ORDER BY work_id LIMIT 65"
        )).bind(operation.id.as_str()).bind(ordinal as i64).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if scopes.len() != count {
            return Err(corrupt_storage());
        }
        let mut work = Vec::with_capacity(scopes.len());
        for scope in scopes {
            let definition =
                super::super::read::definition_on(tx, hub, record_id(&scope, "work_id")?).await?;
            let created = stock::read::operation_on(tx, hub, &definition.created_operation)
                .await?
                .ok_or_else(corrupt_storage)?;
            if created.revision.value() >= operation.revision.value() {
                return Err(corrupt_storage());
            }
            let requirement = definition.at(
                QuantityWork::restore(
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
                .map_err(|_| corrupt_storage())?,
                revision_field(&scope, "before_work_revision")?,
            )?;
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
            work.push(WorkAllocation {
                requirement,
                progress,
            });
        }
        consumed.push(ConsumedStockGroup {
            before: PhysicalStockGroup {
                id: line.group,
                trace: line.trace.id,
                variant: line.variant.id,
                parent: stock::read::optional_id(&row, "parent_group_id")?,
                identification: line.identification.clone(),
                location: line.location.id,
                packs: whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?,
                revision: revision_field(&row, "before_group_revision")?,
                causal_floor: clock(
                    row.try_get("before_causal_floor_unix_s")
                        .map_err(|_| corrupt_storage())?,
                )?,
                work,
            },
            after_revision: revision_field(&row, "after_group_revision")?,
            causal_floor: clock(
                row.try_get("after_causal_floor_unix_s")
                    .map_err(|_| corrupt_storage())?,
            )?,
        });
    }
    let changes = super::super::history::work_changes_bounded_on(
        tx,
        hub,
        &operation.id,
        harvestcircle_domain::hub::departure::HUB_DEPARTURE_WORK_CHANGES,
    )
    .await?;
    if changes.len() != header.changes {
        return Err(corrupt_storage());
    }
    let work_changes = changes
        .into_iter()
        .map(|change| {
            let before = change.before.ok_or_else(corrupt_storage)?;
            let retired = change
                .after
                .quantity
                .no_longer_required()
                .checked_sub(before.quantity.no_longer_required())
                .map_err(|_| corrupt_storage())?;
            Ok(harvestcircle_domain::hub::departure::DepartureWorkChange {
                before,
                after: change.after,
                retired,
            })
        })
        .collect::<Result<Vec<_>, SafeError>>()?;
    let record = DepartureRecord {
        id: header.id,
        operation: identities(&operation.id)?.operation,
        hub,
        stock_revision: operation.revision,
        catalog_revision: operation.catalog_revision,
        actor: operation.actor,
        recorded_at: operation.recorded_at,
        occurred: header.occurred,
        observation: header.observation,
        paper,
        consumed,
        work_changes,
    };
    let command = harvestcircle_domain::hub::departure::DepartureCommand {
        expected_stock_revision: HubRevision::from_value(
            operation
                .revision
                .value()
                .checked_sub(1)
                .ok_or_else(corrupt_storage)?,
        )
        .map_err(|_| corrupt_storage())?,
        expected_catalog_revision: operation.catalog_revision,
        preparation: record.paper.id,
        expected_preparation_revision: record.paper.revision,
        identification: header.identification,
        occurred: record.occurred,
        observation: record.observation.clone(),
    };
    if command.fingerprint(hub, operation.actor, identities(&operation.id)?) != operation.hash {
        return Err(corrupt_storage());
    }
    if departure_result_digest(&operation.hash, &record).map_err(|_| corrupt_storage())?
        != header.digest
    {
        return Err(corrupt_storage());
    }
    events_on(tx, &operation, &record).await?;
    Ok(HubDepartureReceipt {
        operation: operation.id,
        record,
    })
}
async fn events_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &OperationRecord,
    record: &DepartureRecord,
) -> Result<(), SafeError> {
    let events=sqlx::query(concat!(
        "SELECT substr(kind,1,33) AS kind,substr(trace_id,1,17) AS trace_id,substr(group_id,1,17) AS group_id,packs,substr(from_location_id,1,17) AS from_location_id,from_location_revision,",
        "substr(to_location_id,1,17) AS to_location_id,to_location_revision,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s FROM hub_stock_events WHERE operation_id=? ORDER BY sequence LIMIT 65"
    )).bind(operation.id.as_str()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if events.len() != record.consumed.len() {
        return Err(corrupt_storage());
    }
    let lines = &record
        .paper
        .versions
        .last()
        .ok_or_else(corrupt_storage)?
        .material
        .lines;
    for (event, line) in events.into_iter().zip(lines) {
        if event
            .try_get::<String, _>("kind")
            .map_err(|_| corrupt_storage())?
            != "departure"
            || record_id(&event, "trace_id")? != line.trace.id
            || record_id(&event, "group_id")? != line.group
            || whole_packs(event.try_get("packs").map_err(|_| corrupt_storage())?)? != line.packs
            || record_id(&event, "from_location_id")? != line.location.id
            || revision_field(&event, "from_location_revision")? != line.location.revision
            || stock::read::optional_id(&event, "to_location_id")?.is_some()
            || event
                .try_get::<Option<i64>, _>("to_location_revision")
                .map_err(|_| corrupt_storage())?
                .is_some()
            || stock::read::occurrence(&event)? != record.occurred
        {
            return Err(corrupt_storage());
        }
    }
    Ok(())
}
impl Database {
    /// Original recovery deliberately does not require current position/counter state.
    pub async fn hub_departure_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubDepartureReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    match stock::read::operation_on(tx, hub.id(), &operation).await? {
                        Some(original) if original.kind == "departure" => {
                            Ok(Some(original_on(tx, hub.id(), original).await?))
                        }
                        Some(_) => Err(conflict()),
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
