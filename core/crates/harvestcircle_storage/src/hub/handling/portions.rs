use super::super::{reference, stock};
use super::*;
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::portions::SplitCommand;
use harvestcircle_domain::hub::reference::{ReferenceKind, SourceProvenance};
use harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY;
use sqlx::Row;
use stock::read::{
    OperationRecord, optional_id, optional_record_on, record_id, revision_field, text, whole_packs,
};

impl Database {
    pub async fn apply_hub_split(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: SplitCommand,
        now: UnixTimestamp,
    ) -> Result<HubHandlingReceipt, SafeError> {
        self.host()
            .transaction(|transaction| {
                Box::pin(async move { apply_on(transaction, actor, operation, command, now).await })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn apply_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: SplitCommand,
    now: UnixTimestamp,
) -> Result<HubHandlingReceipt, SafeError> {
    let hub = reference::authorize_on(transaction, actor).await?;
    reference::validate_basis_on(transaction, hub.id()).await?;
    let ids = handling_ids(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, ids.group);
    if let Some(original) = stock::read::operation_on(transaction, hub.id(), &operation).await? {
        if original.kind != "split" || original.actor != actor || original.hash != hash {
            return Err(conflict());
        }
        return original_on(transaction, hub.id(), original).await;
    }
    let state = stock::read::state_on(transaction, hub.id()).await?;
    let catalog = reference::load_catalog_on(transaction, hub.id()).await?;
    let source = read::group_on(transaction, hub.id(), command.group, &catalog).await?;
    let before = lineage::load_on(
        transaction,
        hub.id(),
        source.trace,
        &catalog,
        state.revision,
    )
    .await?;
    let captured = before
        .graph
        .capture(&[source.id])
        .map_err(|_| corrupt_storage())?;
    let plan = command
        .plan(&catalog, state.revision, &source, ids.group, now)
        .map_err(handling_error)?;
    if state.event_count as usize >= HUB_STOCK_EVENT_CAPACITY {
        return Err(capacity());
    }
    write::ensure_portion_capacity_on(transaction, &source, &command.identification).await?;
    let recorded = OperationRecord {
        id: operation.clone(),
        kind: "split".into(),
        actor,
        hash,
        catalog_revision: catalog.revision(),
        revision: plan.stock_revision,
        result: plan.portion.id,
        recorded_at: now,
    };
    write::insert_operation_on(transaction, hub.id(), &recorded).await?;
    write::write_groups_on(
        transaction,
        &catalog,
        &operation,
        &source,
        write::GroupChange {
            remaining: Some(&plan.remainder),
            selected: &plan.portion,
            destination: &plan.location,
        },
    )
    .await?;
    sqlx::query("INSERT INTO hub_movement_details (operation_id,selected_group_id,expected_group_revision,source_before_packs,moved_group_id,work_change_count,observation) VALUES (?,?,?,?,?,0,?)")
        .bind(operation.as_str()).bind(source.id.as_bytes().as_slice()).bind(integer(source.revision)?).bind(i64::from(source.packs.value()))
        .bind(plan.portion.id.as_bytes().as_slice()).bind(command.observation.as_str())
        .execute(&mut *transaction).await.map_err(|_|storage_unavailable())?;
    write::event_on(
        transaction,
        &operation,
        write::EventEffect {
            kind: "split",
            prior_events: state.event_count,
            trace: source.trace,
            group: plan.portion.id,
            packs: command.packs,
            from: Some(&plan.location),
            to: Some(&plan.location),
            occurred: command.occurred,
        },
    )
    .await?;
    write::advance_on(transaction, hub.id(), &state, plan.stock_revision).await?;
    if read::group_on(transaction, hub.id(), plan.remainder.id, &catalog).await? != plan.remainder
        || read::group_on(transaction, hub.id(), plan.portion.id, &catalog).await? != plan.portion
    {
        return Err(corrupt_storage());
    }
    let after = lineage::load_on(
        transaction,
        hub.id(),
        source.trace,
        &catalog,
        plan.stock_revision,
    )
    .await?;
    let selected = after
        .graph
        .resolve(&captured)
        .map_err(|_| corrupt_storage())?;
    if selected.packs() != source.packs || selected.groups().len() != 2 {
        return Err(corrupt_storage());
    }
    Ok(HubHandlingReceipt {
        operation,
        kind: HubHandlingKind::Split,
        revision: plan.stock_revision,
        catalog_revision: catalog.revision(),
        trace: source.trace,
        actor,
        recorded_at: now,
        result: HubHandlingResult::Separated(HubSplitResult {
            selected_group: source.id,
            expected_group_revision: source.revision,
            source_before_packs: source.packs,
            location: plan.location,
            packs: command.packs,
            portion_group: plan.portion.id,
            remaining_packs: plan.remainder.packs,
            identification: command.identification,
            occurred: command.occurred,
            observation: command.observation,
        }),
    })
}

pub(super) async fn original_on(
    transaction: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: OperationRecord,
) -> Result<HubHandlingReceipt, SafeError> {
    if operation.kind != "split" {
        return Err(conflict());
    }
    history::single_event_on(transaction, &operation.id).await?;
    let row = sqlx::query(concat!(
        "SELECT substr(d.selected_group_id,1,17) AS selected_group_id,d.expected_group_revision,d.source_before_packs,substr(d.moved_group_id,1,17) AS moved_group_id,d.work_change_count,",
        "substr(CAST(d.observation AS BLOB),1,513) AS observation,e.sequence,substr(e.kind,1,33) AS event_kind,substr(e.trace_id,1,17) AS trace_id,substr(e.group_id,1,17) AS event_group,e.packs,",
        "substr(e.from_location_id,1,17) AS from_location_id,e.from_location_revision,substr(e.to_location_id,1,17) AS to_location_id,e.to_location_revision,",
        "substr(e.occurrence_kind,1,9) AS occurrence_kind,e.occurrence_start_unix_s,e.occurrence_end_unix_s ",
        "FROM hub_movement_details d JOIN hub_stock_events e ON e.operation_id=d.operation_id WHERE d.operation_id=? LIMIT 1"
    )).bind(operation.id.as_str()).fetch_optional(&mut *transaction).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let ids = handling_ids(&operation.id).map_err(|_| corrupt_storage())?;
    let selected = record_id(&row, "selected_group_id")?;
    let portion = record_id(&row, "moved_group_id")?;
    let trace = record_id(&row, "trace_id")?;
    let before = whole_packs(
        row.try_get("source_before_packs")
            .map_err(|_| corrupt_storage())?,
    )?;
    let packs = whole_packs(row.try_get("packs").map_err(|_| corrupt_storage())?)?;
    let sequence: i64 = row.try_get("sequence").map_err(|_| corrupt_storage())?;
    let changes: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_work_changes WHERE operation_id=?")
            .bind(operation.id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| storage_unavailable())?;
    if packs.value() == 0
        || packs.value() >= before.value()
        || selected == portion
        || portion != ids.group
        || portion != operation.result
        || portion != record_id(&row, "event_group")?
        || row
            .try_get::<String, _>("event_kind")
            .map_err(|_| corrupt_storage())?
            != "split"
        || !(1..=HUB_STOCK_EVENT_CAPACITY as i64).contains(&sequence)
        || row
            .try_get::<i64, _>("work_change_count")
            .map_err(|_| corrupt_storage())?
            != 0
        || changes != 0
    {
        return Err(corrupt_storage());
    }
    let identity = sqlx::query(concat!(
        "SELECT substr(i.parent_group_id,1,17) AS parent_group_id,substr(CAST(i.physical_label AS BLOB),1,121) AS physical_label,",
        "substr(CAST(i.observation AS BLOB),1,513) AS observation,substr(i.created_operation_id,1,37) AS created_operation_id,",
        "substr(g.trace_id,1,17) AS trace_id,substr(parent.trace_id,1,17) AS parent_trace FROM hub_group_identification i ",
        "JOIN hub_stock_groups g ON g.group_id=i.group_id JOIN hub_stock_groups parent ON parent.group_id=i.parent_group_id WHERE i.group_id=? LIMIT 1"
    )).bind(portion.as_bytes().as_slice()).fetch_optional(&mut *transaction).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if optional_id(&identity, "parent_group_id")? != Some(selected)
        || record_id(&identity, "trace_id")? != trace
        || record_id(&identity, "parent_trace")? != trace
        || identity
            .try_get::<String, _>("created_operation_id")
            .map_err(|_| corrupt_storage())?
            != operation.id.as_str()
    {
        return Err(corrupt_storage());
    }
    let identification = PhysicalIdentification {
        label: HubLabel::new(text(&identity, "physical_label")?).map_err(|_| corrupt_storage())?,
        observation: SourceProvenance::new(text(&identity, "observation")?)
            .map_err(|_| corrupt_storage())?,
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
    if from != to || from.value.kind() != ReferenceKind::Location {
        return Err(corrupt_storage());
    }
    let occurred = stock::read::occurrence(&row)?;
    occurred
        .validate(operation.recorded_at)
        .map_err(|_| corrupt_storage())?;
    let command = SplitCommand {
        expected_stock_revision: HubRevision::from_value(
            operation
                .revision
                .value()
                .checked_sub(1)
                .ok_or_else(corrupt_storage)?,
        )
        .map_err(|_| corrupt_storage())?,
        expected_catalog_revision: operation.catalog_revision,
        group: selected,
        expected_group_revision: revision_field(&row, "expected_group_revision")?,
        packs,
        identification: identification.clone(),
        occurred,
        observation: SourceProvenance::new(text(&row, "observation")?)
            .map_err(|_| corrupt_storage())?,
    };
    if command.fingerprint(hub, operation.actor, ids.group) != operation.hash {
        return Err(corrupt_storage());
    }
    Ok(HubHandlingReceipt {
        operation: operation.id,
        kind: HubHandlingKind::Split,
        revision: operation.revision,
        catalog_revision: operation.catalog_revision,
        trace,
        actor: operation.actor,
        recorded_at: operation.recorded_at,
        result: HubHandlingResult::Separated(HubSplitResult {
            selected_group: selected,
            expected_group_revision: command.expected_group_revision,
            source_before_packs: before,
            location: from,
            packs,
            portion_group: portion,
            remaining_packs: before.checked_sub(packs).map_err(|_| corrupt_storage())?,
            identification,
            occurred,
            observation: command.observation,
        }),
    })
}
