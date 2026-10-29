use super::super::{reference, stock};
use super::*;
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::movement::{
    DiscrepancyAction, DiscrepancyCommand, HUB_DISCREPANCY_CAPACITY,
};
use harvestcircle_domain::hub::reference::SourceProvenance;
use harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY;
use sqlx::Row;
use stock::read::{OperationRecord, optional_id, record_id, revision_field, text};

pub(in crate::hub) struct DiscrepancyState {
    pub value: Discrepancy,
    pub created: OperationRecord,
    pub resolved: Option<OperationRecord>,
}

impl Database {
    pub async fn apply_hub_discrepancy(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: DiscrepancyCommand,
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
                    apply_on(transaction, actor, operation, command, now).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn apply_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: DiscrepancyCommand,
    now: UnixTimestamp,
) -> Result<HubHandlingReceipt, SafeError> {
    let hub = reference::authorize_on(transaction, actor).await?;
    reference::validate_basis_on(transaction, hub.id()).await?;
    let ids = handling_ids(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, ids.discrepancy);
    let kind = match &command.action {
        DiscrepancyAction::Open { .. } => "discrepancy_open",
        DiscrepancyAction::Resolve { .. } => "discrepancy_resolve",
    };
    if let Some(original) = stock::read::operation_on(transaction, hub.id(), &operation).await? {
        if original.kind != kind || original.hash != hash || original.actor != actor {
            return Err(conflict());
        }
        return original_on(transaction, hub.id(), original).await;
    }
    let state = stock::read::state_on(transaction, hub.id()).await?;
    let catalog = reference::load_catalog_on(transaction, hub.id()).await?;
    if command.expected_stock_revision != state.revision
        || command.expected_catalog_revision != catalog.revision()
    {
        return Err(conflict());
    }
    if state.event_count as usize >= HUB_STOCK_EVENT_CAPACITY {
        return Err(capacity());
    }
    let value = match &command.action {
        DiscrepancyAction::Open {
            trace,
            group,
            observation,
        } => {
            read::trace_on(transaction, hub.id(), *trace).await?;
            if let Some(group) = group {
                let row=sqlx::query("SELECT substr(trace_id,1,17) AS trace_id FROM hub_stock_groups WHERE group_id=? LIMIT 1").bind(group.as_bytes().as_slice())
                    .fetch_optional(&mut *transaction).await.map_err(|_|storage_unavailable())?.ok_or_else(invalid_input)?;
                if record_id(&row, "trace_id")? != *trace {
                    return Err(invalid_input());
                }
            }
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_discrepancies")
                .fetch_one(&mut *transaction)
                .await
                .map_err(|_| storage_unavailable())?;
            if count >= HUB_DISCREPANCY_CAPACITY as i64 {
                return Err(capacity());
            }
            Discrepancy::open(ids.discrepancy, *trace, *group, observation.clone())
        }
        DiscrepancyAction::Resolve {
            discrepancy,
            expected_revision,
            resolution,
        } => {
            let current = value_on(transaction, hub.id(), *discrepancy).await?;
            current
                .value
                .resolve(*expected_revision, resolution.clone())
                .map_err(handling_error)?
        }
    };
    let revision =
        HubRevision::from_value(state.revision.value().checked_add(1).ok_or_else(capacity)?)
            .map_err(|_| capacity())?;
    let recorded = OperationRecord {
        id: operation.clone(),
        kind: kind.to_owned(),
        actor,
        hash,
        catalog_revision: command.expected_catalog_revision,
        revision,
        result: value.id,
        recorded_at: now,
    };
    write::insert_operation_on(transaction, hub.id(), &recorded).await?;
    match &command.action {
        DiscrepancyAction::Open { .. } => {
            sqlx::query("INSERT INTO hub_discrepancies (discrepancy_id,trace_id,group_id,observation,revision,resolution,created_operation_id,resolved_operation_id) VALUES (?,?,?,?,1,NULL,?,NULL)")
                .bind(value.id.as_bytes().as_slice()).bind(value.trace.as_bytes().as_slice()).bind(value.group.as_ref().map(|id|id.as_bytes().as_slice())).bind(value.observation.as_str()).bind(operation.as_str())
                .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
        }
        DiscrepancyAction::Resolve {
            expected_revision, ..
        } => {
            let changed=sqlx::query("UPDATE hub_discrepancies SET revision=?,resolution=?,resolved_operation_id=? WHERE discrepancy_id=? AND revision=? AND resolution IS NULL AND resolved_operation_id IS NULL")
                .bind(integer(value.revision)?).bind(value.resolution.as_ref().map(|text|text.as_str())).bind(operation.as_str()).bind(value.id.as_bytes().as_slice()).bind(integer(*expected_revision)?)
                .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
            if changed.rows_affected() != 1 {
                return Err(conflict());
            }
        }
    }
    let row = sqlx::query(
        "SELECT substr(root_group_id,1,17) AS group_id FROM hub_receipts WHERE trace_id=? LIMIT 1",
    )
    .bind(value.trace.as_bytes().as_slice())
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| storage_unavailable())?;
    write::event_on(
        transaction,
        &operation,
        write::EventEffect {
            kind,
            prior_events: state.event_count,
            trace: value.trace,
            group: record_id(&row, "group_id")?,
            packs: WholePacks::from_value(0).map_err(|_| corrupt_storage())?,
            from: None,
            to: None,
            occurred: Occurrence::Unknown,
        },
    )
    .await?;
    write::advance_on(transaction, hub.id(), &state, revision).await?;
    Ok(HubHandlingReceipt {
        operation,
        kind: HubHandlingKind::parse(kind)?,
        revision,
        catalog_revision: catalog.revision(),
        trace: value.trace,
        actor,
        recorded_at: now,
        result: HubHandlingResult::DiscrepancyChanged(value),
    })
}

async fn creation_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
) -> Result<DiscrepancyState, SafeError> {
    let row=sqlx::query("SELECT substr(discrepancy_id,1,17) AS discrepancy_id,substr(trace_id,1,17) AS trace_id,substr(group_id,1,17) AS group_id,substr(CAST(observation AS BLOB),1,513) AS observation,substr(created_operation_id,1,37) AS created_operation_id FROM hub_discrepancies WHERE discrepancy_id=? LIMIT 1")
        .bind(id.as_bytes().as_slice()).fetch_optional(&mut *transaction).await.map_err(|_|storage_unavailable())?.ok_or_else(invalid_input)?;
    let created_id = DurableRequestId::parse(
        row.try_get::<String, _>("created_operation_id")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let created = stock::read::operation_on(transaction, hub, &created_id)
        .await?
        .ok_or_else(corrupt_storage)?;
    if created.kind != "discrepancy_open"
        || created.result != id
        || handling_ids(&created.id)?.discrepancy != id
    {
        return Err(corrupt_storage());
    }

    let value = Discrepancy::open(
        record_id(&row, "discrepancy_id")?,
        record_id(&row, "trace_id")?,
        optional_id(&row, "group_id")?,
        SourceProvenance::new(text(&row, "observation")?).map_err(|_| corrupt_storage())?,
    );
    verify_intent(
        hub,
        &created,
        DiscrepancyAction::Open {
            trace: value.trace,
            group: value.group,
            observation: value.observation.clone(),
        },
    )?;
    history::validate_nonphysical_event_on(transaction, &created, value.trace).await?;
    Ok(DiscrepancyState {
        value,
        created,
        resolved: None,
    })
}

pub(in crate::hub) async fn value_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
) -> Result<DiscrepancyState, SafeError> {
    let base = creation_on(transaction, hub, id).await?;
    let created = base.created;
    let row=sqlx::query("SELECT revision,substr(CAST(resolution AS BLOB),1,513) AS resolution,substr(resolved_operation_id,1,37) AS resolved_operation_id FROM hub_discrepancies WHERE discrepancy_id=? LIMIT 1")
        .bind(id.as_bytes().as_slice()).fetch_one(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    let revision = revision_field(&row, "revision")?;
    let resolution: Option<Vec<u8>> = row.try_get("resolution").map_err(|_| corrupt_storage())?;
    let resolved_id: Option<String> = row
        .try_get("resolved_operation_id")
        .map_err(|_| corrupt_storage())?;
    let (resolution, resolved) = match (revision.value(), resolution, resolved_id) {
        (1, None, None) => (None, None),
        (2, Some(text), Some(operation)) => {
            let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
            let operation = stock::read::operation_on(transaction, hub, &operation)
                .await?
                .ok_or_else(corrupt_storage)?;
            if operation.kind != "discrepancy_resolve"
                || operation.result != id
                || operation.revision.value() <= created.revision.value()
            {
                return Err(corrupt_storage());
            }
            (
                Some(
                    SourceProvenance::new(String::from_utf8(text).map_err(|_| corrupt_storage())?)
                        .map_err(|_| corrupt_storage())?,
                ),
                Some(operation),
            )
        }
        _ => return Err(corrupt_storage()),
    };
    let value = Discrepancy {
        revision,
        resolution,
        ..base.value
    };
    if let Some(operation) = &resolved {
        verify_intent(
            hub,
            operation,
            DiscrepancyAction::Resolve {
                discrepancy: value.id,
                expected_revision: HubRevision::INITIAL,
                resolution: value.resolution.clone().ok_or_else(corrupt_storage)?,
            },
        )?;
        history::validate_nonphysical_event_on(transaction, operation, value.trace).await?;
    }
    Ok(DiscrepancyState {
        value,
        created,
        resolved,
    })
}

pub(super) async fn original_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: OperationRecord,
) -> Result<HubHandlingReceipt, SafeError> {
    let kind = HubHandlingKind::parse(&operation.kind)?;
    if !matches!(
        kind,
        HubHandlingKind::OpenDiscrepancy | HubHandlingKind::ResolveDiscrepancy
    ) {
        return Err(conflict());
    }
    let state = if kind == HubHandlingKind::OpenDiscrepancy {
        creation_on(transaction, hub, operation.result).await?
    } else {
        value_on(transaction, hub, operation.result).await?
    };
    let (action, value) = match kind {
        HubHandlingKind::OpenDiscrepancy => {
            if state.created.id != operation.id {
                return Err(corrupt_storage());
            }
            (
                DiscrepancyAction::Open {
                    trace: state.value.trace,
                    group: state.value.group,
                    observation: state.value.observation.clone(),
                },
                Discrepancy {
                    revision: HubRevision::INITIAL,
                    resolution: None,
                    ..state.value
                },
            )
        }
        HubHandlingKind::ResolveDiscrepancy => {
            if state.resolved.as_ref().map(|resolved| &resolved.id) != Some(&operation.id) {
                return Err(corrupt_storage());
            }
            (
                DiscrepancyAction::Resolve {
                    discrepancy: state.value.id,
                    expected_revision: HubRevision::INITIAL,
                    resolution: state.value.resolution.clone().ok_or_else(corrupt_storage)?,
                },
                state.value,
            )
        }
        _ => return Err(corrupt_storage()),
    };
    verify_intent(hub, &operation, action)?;
    history::validate_nonphysical_event_on(transaction, &operation, value.trace).await?;
    Ok(HubHandlingReceipt {
        operation: operation.id,
        kind,
        revision: operation.revision,
        catalog_revision: operation.catalog_revision,
        trace: value.trace,
        actor: operation.actor,
        recorded_at: operation.recorded_at,
        result: HubHandlingResult::DiscrepancyChanged(value),
    })
}

fn verify_intent(
    hub: HubId,
    operation: &OperationRecord,
    action: DiscrepancyAction,
) -> Result<(), SafeError> {
    let stock = HubRevision::from_value(
        operation
            .revision
            .value()
            .checked_sub(1)
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let command = DiscrepancyCommand {
        expected_stock_revision: stock,
        expected_catalog_revision: operation.catalog_revision,
        action,
    };
    if command.fingerprint(
        hub,
        operation.actor,
        handling_ids(&operation.id)?.discrepancy,
    ) != operation.hash
    {
        return Err(corrupt_storage());
    }
    Ok(())
}
