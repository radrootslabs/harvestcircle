//! Present counted stock is an explicit entry, never a reconstructed delivery.
use super::*;
use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::hub::{handling, reference};
use harvestcircle_domain::hub::movement::{HUB_GROUP_CAPACITY, PhysicalIdentification};
use harvestcircle_domain::hub::reference::SourceProvenance;
use harvestcircle_domain::hub::stock::{
    OpeningCommand, OpeningError, OpeningHistory, OpeningIdentities, PopulationOrigin,
};
use harvestcircle_domain::hub::{HubId, HubOwnership};
use harvestcircle_domain::{PublicKey, UnixTimestamp};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;

fn identities(operation: &DurableRequestId) -> Result<OpeningIdentities, SafeError> {
    let id =
        HubRecordId::from_hex(&operation.as_str().replace('-', "")).map_err(|_| invalid_input())?;
    OpeningIdentities::from_operation_bytes(id).map_err(|_| invalid_input())
}

impl Database {
    pub async fn apply_hub_opening(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: OpeningCommand,
        now: UnixTimestamp,
    ) -> Result<HubOpeningReceipt, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    let ids = identities(&operation)?;
                    let hash = command.fingerprint(hub.id(), actor, ids);
                    if let Some(original) =
                        super::read::operation_on(tx, hub.id(), &operation).await?
                    {
                        if original.kind != "opening"
                            || original.hash != hash
                            || original.actor != actor
                        {
                            return Err(conflict());
                        }
                        return original_on(tx, hub.id(), original).await;
                    }
                    handling::treatment::capacity::ensure_on(
                        tx,
                        actor,
                        &operation,
                        handling::treatment::capacity::Reservation::Ordinary(1),
                    )
                    .await?;
                    apply_on(tx, hub, operation, command, ids, hash, now).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    pub async fn hub_opening_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubOpeningReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    match super::read::operation_on(tx, hub.id(), &operation).await? {
                        Some(original) => Ok(Some(original_on(tx, hub.id(), original).await?)),
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubOwnership,
    operation: DurableRequestId,
    command: OpeningCommand,
    ids: OpeningIdentities,
    hash: [u8; 32],
    now: UnixTimestamp,
) -> Result<HubOpeningReceipt, SafeError> {
    let state = super::read::state_on(tx, hub.id()).await?;
    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
    let plan = command
        .plan(&catalog, state.revision, ids, hub.owner(), now)
        .map_err(opening_error)?;
    let population_count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_receipts")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    let group_count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_stock_groups")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if !(0..=HUB_TRACE_CAPACITY as i64).contains(&population_count)
        || !(0..=HUB_GROUP_CAPACITY as i64).contains(&group_count)
    {
        return Err(corrupt_storage());
    }
    if population_count == HUB_TRACE_CAPACITY as i64 || group_count == HUB_GROUP_CAPACITY as i64 {
        return Err(capacity());
    }
    let duplicates: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_group_identification i WHERE i.physical_label=? AND EXISTS(SELECT 1 FROM hub_stock_positions p WHERE p.group_id=i.group_id AND p.packs>0)")
        .bind(command.identification.label.as_str()).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
    if duplicates != 0 {
        return Err(conflict());
    }
    let record = super::read::OperationRecord {
        id: operation.clone(),
        kind: "opening".into(),
        actor: hub.owner(),
        hash,
        catalog_revision: command.expected_catalog_revision,
        revision: plan.stock_revision,
        result: ids.population,
        recorded_at: now,
    };
    handling::write::insert_operation_on(tx, hub.id(), &record).await?;
    super::write::persist_source_on(tx, hub, &operation, &plan.population, ids.source).await?;
    super::write::insert_population_on(tx, &operation, &plan.population, ids.group).await?;
    let (history_kind, history_reference) = match &command.history {
        OpeningHistory::Unknown => ("unknown", None),
        OpeningHistory::ExternalReference(reference) => {
            ("external_reference", Some(reference.as_str()))
        }
    };
    sqlx::query("INSERT INTO hub_counted_openings (population_id,history_kind,history_reference) VALUES (?,?,?)")
        .bind(ids.population.as_bytes().as_slice()).bind(history_kind).bind(history_reference)
        .execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
    sqlx::query("INSERT INTO hub_stock_groups (group_id,trace_id,revision) VALUES (?,?,1)")
        .bind(ids.group.as_bytes().as_slice())
        .bind(ids.population.as_bytes().as_slice())
        .execute(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    sqlx::query("INSERT INTO hub_stock_positions (group_id,location_id,location_revision,packs,revision) VALUES (?,?,?,?,1)")
        .bind(ids.group.as_bytes().as_slice()).bind(command.location.as_bytes().as_slice())
        .bind(i64::try_from(plan.population.initial_location.revision.value()).map_err(|_| corrupt_storage())?)
        .bind(i64::from(command.packs.value())).execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
    sqlx::query("INSERT INTO hub_group_identification (group_id,parent_group_id,physical_label,observation,created_operation_id) VALUES (?,NULL,?,?,?)")
        .bind(ids.group.as_bytes().as_slice()).bind(command.identification.label.as_str())
        .bind(command.identification.observation.as_str()).bind(operation.as_str())
        .execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
    handling::write::event_on(
        tx,
        &operation,
        handling::write::EventEffect {
            kind: "opening",
            prior_events: state.event_count,
            trace: ids.population,
            group: ids.group,
            packs: command.packs,
            from: None,
            to: Some(&plan.population.initial_location),
            occurred: command.counted,
        },
    )
    .await?;
    handling::write::advance_many_on(tx, hub.id(), &state, plan.stock_revision, 1).await?;
    let restored = original_on(tx, hub.id(), record).await?;
    if restored.original != plan {
        return Err(corrupt_storage());
    }
    Ok(restored)
}

pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: super::read::OperationRecord,
) -> Result<HubOpeningReceipt, SafeError> {
    if operation.kind != "opening" {
        return Err(conflict());
    }
    let fields = super::read::population_fields_on(tx, hub, &operation).await?;
    if fields.origin_kind != "opening"
        || fields.supplier.is_some()
        || fields.producer.is_some()
        || fields.planned_location.is_some()
    {
        return Err(corrupt_storage());
    }
    let row = sqlx::query(concat!(
        "SELECT substr(e.origin_kind,1,33) AS origin_kind,substr(e.history_kind,1,33) AS history_kind,",
        "substr(CAST(e.history_reference AS BLOB),1,513) AS history_reference,",
        "substr(i.parent_group_id,1,17) AS parent_id,substr(CAST(i.physical_label AS BLOB),1,121) AS label,",
        "substr(CAST(i.observation AS BLOB),1,513) AS identification_observation,",
        "substr(i.created_operation_id,1,37) AS created_operation FROM hub_counted_openings e ",
        "JOIN hub_group_identification i ON i.group_id=? WHERE e.population_id=? LIMIT 1"
    )).bind(fields.group.as_bytes().as_slice()).bind(fields.trace.as_bytes().as_slice())
        .fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if row
        .try_get::<String, _>("origin_kind")
        .map_err(|_| corrupt_storage())?
        != "opening"
        || super::read::optional_id(&row, "parent_id")?.is_some()
        || row
            .try_get::<String, _>("created_operation")
            .map_err(|_| corrupt_storage())?
            != operation.id.as_str()
    {
        return Err(corrupt_storage());
    }
    let history_reference = row
        .try_get::<Option<Vec<u8>>, _>("history_reference")
        .map_err(|_| corrupt_storage())?;
    let history = match (
        row.try_get::<String, _>("history_kind")
            .map_err(|_| corrupt_storage())?
            .as_str(),
        history_reference,
    ) {
        ("unknown", None) => OpeningHistory::Unknown,
        ("external_reference", Some(bytes)) => OpeningHistory::ExternalReference(
            SourceProvenance::new(String::from_utf8(bytes).map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())?,
        ),
        _ => return Err(corrupt_storage()),
    };
    let command = OpeningCommand {
        expected_stock_revision: HubRevision::from_value(operation.revision.value() - 1)
            .map_err(|_| corrupt_storage())?,
        expected_catalog_revision: operation.catalog_revision,
        variant: fields.variant,
        location: fields.initial_location,
        packs: fields.packs,
        identification: PhysicalIdentification {
            label: HubLabel::new(super::read::text(&row, "label")?)
                .map_err(|_| corrupt_storage())?,
            observation: SourceProvenance::new(super::read::text(
                &row,
                "identification_observation",
            )?)
            .map_err(|_| corrupt_storage())?,
        },
        source: fields.source,
        history,
        counted: fields.occurred,
        observation: fields.observation,
    };
    let ids = identities(&operation.id).map_err(|_| corrupt_storage())?;
    if fields.trace != ids.population
        || fields.group != ids.group
        || operation.result != ids.population
        || command.fingerprint(hub, operation.actor, ids) != operation.hash
    {
        return Err(corrupt_storage());
    }
    let original = command
        .plan(
            &fields.catalog,
            command.expected_stock_revision,
            ids,
            operation.actor,
            operation.recorded_at,
        )
        .map_err(|_| corrupt_storage())?;
    if !matches!(
        original.population.origin,
        PopulationOrigin::CountedOpening { .. }
    ) {
        return Err(corrupt_storage());
    }
    let (kind, start, end) = super::write::occurrence_fields(original.population.occurred);
    let event = sqlx::query(concat!(
        "SELECT count(*) AS n,count(CASE WHEN kind='opening' AND trace_id=? AND group_id=? AND packs=? ",
        "AND from_location_id IS NULL AND from_location_revision IS NULL AND to_location_id=? AND to_location_revision=? ",
        "AND occurrence_kind=? AND occurrence_start_unix_s IS ? AND occurrence_end_unix_s IS ? THEN 1 END) AS correct ",
        "FROM hub_stock_events WHERE operation_id=?"
    )).bind(ids.population.as_bytes().as_slice()).bind(ids.group.as_bytes().as_slice())
        .bind(i64::from(original.population.original_packs.value())).bind(original.group.location.as_bytes().as_slice())
        .bind(i64::try_from(original.population.initial_location.revision.value()).map_err(|_| corrupt_storage())?)
        .bind(kind).bind(start).bind(end).bind(operation.id.as_str())
        .fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
    if event
        .try_get::<i64, _>("n")
        .map_err(|_| corrupt_storage())?
        != 1
        || event
            .try_get::<i64, _>("correct")
            .map_err(|_| corrupt_storage())?
            != 1
    {
        return Err(corrupt_storage());
    }
    Ok(HubOpeningReceipt {
        operation: operation.id,
        original,
    })
}

fn opening_error(error: OpeningError) -> SafeError {
    match error {
        OpeningError::StaleRevision
        | OpeningError::Stock(
            harvestcircle_domain::hub::stock::StockError::SourceIdentityConflict,
        ) => conflict(),
        OpeningError::RevisionExhausted => capacity(),
        _ => invalid_input(),
    }
}

#[cfg(test)]
pub(in crate::hub) mod tests;
