use super::*;

impl Database {
    pub async fn hub_count_observation_operation(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubCountObservationReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    match stock::read::operation_on(tx, hub.id(), &operation).await? {
                        Some(record) => Ok(Some(original_on(tx, hub.id(), record).await?)),
                        None => Ok(None),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(in crate::hub) async fn by_id_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    id: HubRecordId,
) -> Result<HubCountObservationReceipt, SafeError> {
    let operation: Option<String> = sqlx::query_scalar(
        "SELECT substr(operation_id,1,37) FROM hub_count_observations WHERE count_id=? LIMIT 1",
    )
    .bind(id.as_bytes().as_slice())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let operation = DurableRequestId::parse(operation.ok_or_else(stock::invalid_input)?)
        .map_err(|_| corrupt_storage())?;
    let record = stock::read::operation_on(tx, hub, &operation)
        .await?
        .ok_or_else(corrupt_storage)?;
    let receipt = original_on(tx, hub, record).await?;
    if receipt.original.id != id {
        return Err(corrupt_storage());
    }
    Ok(receipt)
}

pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: stock::read::OperationRecord,
) -> Result<HubCountObservationReceipt, SafeError> {
    if operation.kind != "count_observe" {
        return Err(stock::conflict());
    }
    let row = sqlx::query(concat!(
        "SELECT substr(count_id,1,17) AS count_id,substr(trace_id,1,17) AS trace_id,substr(group_id,1,17) AS group_id,group_revision,",
        "substr(product_id,1,17) AS product_id,product_revision,substr(variant_id,1,17) AS variant_id,variant_revision,",
        "substr(location_id,1,17) AS location_id,location_revision,recorded_packs,counted_packs,latest_physical_recorded_at_unix_s,",
        "substr(previous_reconciliation_id,1,17) AS previous_reconciliation_id,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,",
        "substr(CAST(observation AS BLOB),1,513) AS observation,substr(result_hash,1,33) AS result_hash ",
        "FROM hub_count_observations WHERE operation_id=? LIMIT 1"
    )).bind(operation.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let product = reference::record_at_on(
        tx,
        hub,
        record_id(&row, "product_id")?,
        revision_field(&row, "product_revision")?,
    )
    .await?;
    let variant = reference::record_at_on(
        tx,
        hub,
        record_id(&row, "variant_id")?,
        revision_field(&row, "variant_revision")?,
    )
    .await?;
    let location = reference::record_at_on(
        tx,
        hub,
        record_id(&row, "location_id")?,
        revision_field(&row, "location_revision")?,
    )
    .await?;
    let original = CountObservation {
        id: record_id(&row, "count_id")?,
        operation: operation_id(&operation.id).map_err(|_| corrupt_storage())?,
        recorded_revision: operation.revision,
        scope: CountScope {
            hub,
            stock_revision: HubRevision::from_value(operation.revision.value() - 1)
                .map_err(|_| corrupt_storage())?,
            catalog_revision: operation.catalog_revision,
            trace: record_id(&row, "trace_id")?,
            group: record_id(&row, "group_id")?,
            group_revision: revision_field(&row, "group_revision")?,
            product,
            variant,
            location,
            recorded_packs: whole_packs(
                row.try_get("recorded_packs")
                    .map_err(|_| corrupt_storage())?,
            )?,
            latest_physical_recorded_at: stock::read::timestamp(
                row.try_get("latest_physical_recorded_at_unix_s")
                    .map_err(|_| corrupt_storage())?,
            )?,
            previous_reconciliation: stock::read::optional_id(&row, "previous_reconciliation_id")?,
        },
        counted_packs: whole_packs(
            row.try_get("counted_packs")
                .map_err(|_| corrupt_storage())?,
        )?,
        cutoff: stock::read::occurrence(&row)?,
        actor: operation.actor,
        recorded_at: operation.recorded_at,
        observation: SourceProvenance::new(stock::read::text(&row, "observation")?)
            .map_err(|_| corrupt_storage())?,
    };
    let command = CountObserveCommand {
        expected_stock_revision: original.scope.stock_revision,
        expected_catalog_revision: original.scope.catalog_revision,
        group: original.scope.group,
        expected_group_revision: original.scope.group_revision,
        counted_packs: original.counted_packs,
        cutoff: original.cutoff,
        observation: original.observation.clone(),
    };
    let digest: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    if operation.result != original.id
        || command.fingerprint(hub, operation.actor, original.operation) != operation.hash
        || digest.as_slice()
            != count_observation_result_digest(&operation.hash, &original)
                .map_err(|_| corrupt_storage())?
    {
        return Err(corrupt_storage());
    }
    let (kind, start, end) = stock::write::occurrence_fields(original.cutoff);
    let event = sqlx::query(concat!(
        "SELECT count(*) AS n,count(CASE WHEN kind='count_observe' AND trace_id=? AND group_id=? AND packs=? ",
        "AND from_location_id IS NULL AND from_location_revision IS NULL AND to_location_id IS NULL AND to_location_revision IS NULL ",
        "AND occurrence_kind=? AND occurrence_start_unix_s IS ? AND occurrence_end_unix_s IS ? THEN 1 END) AS correct ",
        "FROM hub_stock_events WHERE operation_id=?"
    )).bind(original.scope.trace.as_bytes().as_slice()).bind(original.scope.group.as_bytes().as_slice())
        .bind(i64::from(original.counted_packs.value())).bind(kind).bind(start).bind(end).bind(operation.id.as_str())
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
    Ok(HubCountObservationReceipt {
        operation: operation.id,
        original,
    })
}

/// Latest physical recording in the selected portion's bounded causal ancestry.
/// Sibling activity after separation cannot silently become this portion's cutoff.
pub(super) async fn latest_physical_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    group: HubRecordId,
) -> Result<UnixTimestamp, SafeError> {
    let row = sqlx::query(concat!(
        "WITH RECURSIVE lineage(group_id,ceiling,depth) AS (SELECT ?,9223372036854775807,0 UNION ALL ",
        "SELECT i.parent_group_id,min(l.ceiling,o.stock_revision-1),l.depth+1 FROM lineage l ",
        "JOIN hub_group_identification i ON i.group_id=l.group_id JOIN hub_stock_operations o ON o.operation_id=i.created_operation_id ",
        "WHERE i.parent_group_id IS NOT NULL AND l.depth<256) ",
        "SELECT max(o.recorded_at_unix_s) AS latest,",
        "(SELECT count(*) FROM lineage l JOIN hub_group_identification i ON i.group_id=l.group_id WHERE l.depth=256 AND i.parent_group_id IS NOT NULL) AS truncated ",
        "FROM lineage l JOIN hub_stock_events e ON e.group_id=l.group_id AND e.kind IN ('receipt','opening','move','split','count_post') ",
        "JOIN hub_stock_operations o ON o.operation_id=e.operation_id WHERE o.stock_revision<=l.ceiling"
    )).bind(group.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
    if row
        .try_get::<i64, _>("truncated")
        .map_err(|_| corrupt_storage())?
        != 0
    {
        return Err(corrupt_storage());
    }
    stock::read::timestamp(
        row.try_get::<Option<i64>, _>("latest")
            .map_err(|_| corrupt_storage())?
            .ok_or_else(corrupt_storage)?,
    )
}

pub(super) async fn previous_reconciliation_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    group: HubRecordId,
) -> Result<Option<HubRecordId>, SafeError> {
    let bytes: Option<Vec<u8>> = sqlx::query_scalar(concat!(
        "SELECT substr(o.result_id,1,17) FROM hub_stock_operations o JOIN hub_stock_events e ON e.operation_id=o.operation_id ",
        "WHERE o.kind='count_post' AND e.group_id=? ORDER BY o.stock_revision DESC LIMIT 1"
    )).bind(group.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?;
    bytes
        .map(|bytes| {
            HubRecordId::from_bytes(bytes.try_into().map_err(|_| corrupt_storage())?)
                .map_err(|_| corrupt_storage())
        })
        .transpose()
}
