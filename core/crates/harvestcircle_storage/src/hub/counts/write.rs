use super::*;

impl Database {
    pub async fn observe_hub_count(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
        command: CountObserveCommand,
        now: UnixTimestamp,
    ) -> Result<HubCountObservationReceipt, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    let hash = command.fingerprint(hub.id(), actor, operation_id(&operation)?);
                    if let Some(original) =
                        stock::read::operation_on(tx, hub.id(), &operation).await?
                    {
                        if original.kind != "count_observe"
                            || original.hash != hash
                            || original.actor != actor
                        {
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
                    observe_on(tx, hub.id(), actor, operation, command, hash, now).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn observe_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: DurableRequestId,
    command: CountObserveCommand,
    hash: [u8; 32],
    now: UnixTimestamp,
) -> Result<HubCountObservationReceipt, SafeError> {
    let state = stock::read::state_on(tx, hub).await?;
    let catalog = reference::load_catalog_on(tx, hub).await?;
    let group = handling::read::group_on(tx, hub, command.group, &catalog).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_count_observations")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if !(0..=HUB_COUNT_CAPACITY as i64).contains(&count) {
        return Err(corrupt_storage());
    }
    if count == HUB_COUNT_CAPACITY as i64 {
        return Err(stock::capacity());
    }
    let latest_physical_recorded_at = read::latest_physical_on(tx, group.id).await?;
    let context = CountContext {
        hub,
        stock_revision: state.revision,
        catalog: &catalog,
        group: &group,
        latest_physical_recorded_at,
        previous_reconciliation: read::previous_reconciliation_on(tx, group.id).await?,
        latest_observation: read::latest_observation_on(tx, group.id).await?,
        // Observation recording never tests these flags or authorizes reconciliation.
        // The posting adapter must load current actual/intent facts independently.
        unresolved_actual: false,
        uncertain_intent: false,
    };
    let original = command
        .plan(&context, operation_id(&operation)?, actor, now)
        .map_err(count_error)?;
    let digest =
        count_observation_result_digest(&hash, &original).map_err(|_| corrupt_storage())?;
    let record = stock::read::OperationRecord {
        id: operation.clone(),
        kind: "count_observe".into(),
        actor,
        hash,
        catalog_revision: catalog.revision(),
        revision: original.recorded_revision,
        result: original.id,
        recorded_at: now,
    };
    handling::write::insert_operation_on(tx, hub, &record).await?;
    let scope = &original.scope;
    let integer = |r: HubRevision| i64::try_from(r.value()).map_err(|_| corrupt_storage());
    let (kind, start, end) = stock::write::occurrence_fields(original.cutoff);
    sqlx::query(concat!(
        "INSERT INTO hub_count_observations (count_id,operation_id,trace_id,group_id,group_revision,",
        "product_id,product_revision,variant_id,variant_revision,location_id,location_revision,",
        "recorded_packs,counted_packs,latest_physical_recorded_at_unix_s,previous_reconciliation_id,",
        "occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,observation,result_hash) ",
        "VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"
    )).bind(original.id.as_bytes().as_slice()).bind(operation.as_str())
        .bind(scope.trace.as_bytes().as_slice()).bind(scope.group.as_bytes().as_slice()).bind(integer(scope.group_revision)?)
        .bind(scope.product.id.as_bytes().as_slice()).bind(integer(scope.product.revision)?)
        .bind(scope.variant.id.as_bytes().as_slice()).bind(integer(scope.variant.revision)?)
        .bind(scope.location.id.as_bytes().as_slice()).bind(integer(scope.location.revision)?)
        .bind(i64::from(scope.recorded_packs.value())).bind(i64::from(original.counted_packs.value()))
        .bind(scope.latest_physical_recorded_at.as_seconds())
        .bind(scope.previous_reconciliation.as_ref().map(|id| id.as_bytes().as_slice()))
        .bind(kind).bind(start).bind(end).bind(original.observation.as_str()).bind(digest.as_slice())
        .execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
    handling::write::event_on(
        tx,
        &operation,
        handling::write::EventEffect {
            kind: "count_observe",
            prior_events: state.event_count,
            trace: scope.trace,
            group: scope.group,
            packs: original.counted_packs,
            from: None,
            to: None,
            occurred: original.cutoff,
        },
    )
    .await?;
    handling::write::advance_many_on(tx, hub, &state, original.recorded_revision, 1).await?;
    let restored = read::original_on(tx, hub, record).await?;
    if restored.original != original {
        return Err(corrupt_storage());
    }
    Ok(restored)
}
