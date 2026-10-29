use super::*;

macro_rules! linked_predicate { () => { concat!(
    "EXISTS(SELECT 1 FROM hub_stock_events e WHERE e.operation_id=o.operation_id AND e.trace_id=?) OR ",
    "EXISTS(SELECT 1 FROM hub_dispatch_changes d LEFT JOIN hub_dispatch_changes prior ON prior.preparation_id=d.preparation_id AND prior.record_revision=d.expected_record_revision JOIN hub_dispatch_material_lines m ON m.operation_id=d.material_operation_id OR m.operation_id=prior.material_operation_id WHERE d.operation_id=o.operation_id AND m.trace_id=?)"
) }; }
impl Database {
    pub async fn hub_trace_history(
        &self,
        actor: PublicKey,
        query: HubTraceHistoryQuery,
    ) -> Result<HubTraceHistoryPage, SafeError> {
        query.validate()?;
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let (hub, state, catalog) = context_on(
                        tx,
                        actor,
                        query.expected_revision,
                        query.expected_catalog_revision,
                    )
                    .await?;
                    let _ = super::super::read::trace_on(tx, hub, query.trace).await?;
                    // Keep count and page predicates one static checked SQL contract.
                    let count: i64 = sqlx::query_scalar(concat!(
                        "SELECT count(*) FROM hub_stock_operations o WHERE ",
                        linked_predicate!()
                    ))
                    .bind(query.trace.as_bytes().as_slice())
                    .bind(query.trace.as_bytes().as_slice())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| storage_unavailable())?;
                    let total = super::super::query::total(
                        count,
                        harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY,
                        query.offset,
                    )?;
                    let operations: Vec<String> = sqlx::query_scalar(concat!(
                        "SELECT substr(o.operation_id,1,37) FROM hub_stock_operations o WHERE ",
                        linked_predicate!(),
                        " ",
                        "ORDER BY o.stock_revision LIMIT ? OFFSET ?"
                    ))
                    .bind(query.trace.as_bytes().as_slice())
                    .bind(query.trace.as_bytes().as_slice())
                    .bind(i64::from(query.limit))
                    .bind(i64::from(query.offset))
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(|_| storage_unavailable())?;
                    let mut rows = Vec::with_capacity(operations.len());
                    for operation in operations {
                        let operation =
                            DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
                        let operation = stock::read::operation_on(tx, hub, &operation)
                            .await?
                            .ok_or_else(corrupt_storage)?;
                        rows.push(entry_on(tx, hub, operation).await?);
                    }
                    Ok(HubTraceHistoryPage {
                        trace: query.trace,
                        revision: state.revision,
                        catalog_revision: catalog.revision(),
                        offset: query.offset,
                        total,
                        more: query.offset
                            + u32::try_from(rows.len()).map_err(|_| corrupt_storage())?
                            < total,
                        rows,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
async fn entry_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    operation: stock::read::OperationRecord,
) -> Result<HubTraceHistoryEntry, SafeError> {
    let id = operation.id.clone();
    let revision = operation.revision;
    let actor = operation.actor;
    let recorded_at = operation.recorded_at;
    let result = operation.result;
    let (kind, occurred, observation) = match operation.kind.as_str() {
        "count_post" => {
            let original = crate::hub::counts::posting::original_on(tx, hub, operation)
                .await?
                .original;
            (
                HubTraceHistoryKind::CountReconciled,
                Some(Occurrence::Known(original.confirmed_on_hand_at)),
                original.reason,
            )
        }
        "count_observe" => {
            let original = crate::hub::counts::read::original_on(tx, hub, operation).await?;
            (
                HubTraceHistoryKind::CountObserved,
                Some(original.original.cutoff),
                original.original.observation,
            )
        }
        "opening" => {
            let original = stock::opening::original_on(tx, hub, operation).await?;
            (
                HubTraceHistoryKind::CountedOpening,
                Some(original.original.population.occurred),
                original.original.population.observation,
            )
        }
        "receipt" => {
            let original = stock::read::receipt_on(tx, hub, operation).await?;
            (
                HubTraceHistoryKind::Receipt,
                Some(original.original.trace.occurred),
                original.original.trace.observation,
            )
        }
        "departure" => {
            let original = super::super::departure::read::original_on(tx, hub, operation).await?;
            (
                HubTraceHistoryKind::Departure,
                Some(original.record.occurred),
                original.record.observation,
            )
        }
        "dispatch_prepare" | "dispatch_revise" | "dispatch_issue" | "dispatch_replace"
        | "dispatch_unused" => {
            let original = super::super::dispatch::read::original_on(tx, hub, operation).await?;
            (
                HubTraceHistoryKind::Dispatch(original.kind),
                None,
                original.record.updated.observation,
            )
        }
        _ => {
            let original = super::super::history::original_on(tx, hub, operation).await?;
            let kind = HubTraceHistoryKind::Handling(original.kind);
            let (occurred, observation) = match original.result {
                HubHandlingResult::Moved(value) => (Some(value.occurred), value.observation),
                HubHandlingResult::Separated(value) => (Some(value.occurred), value.observation),
                HubHandlingResult::WorkChanged(value) => (None, value.observation),
                HubHandlingResult::DiscrepancyChanged(value) => (
                    None,
                    if original.kind == HubHandlingKind::ResolveDiscrepancy {
                        value.resolution.ok_or_else(corrupt_storage)?
                    } else {
                        value.observation
                    },
                ),
                HubHandlingResult::Treatment(value) => {
                    use harvestcircle_domain::hub::treatment::TreatmentChange;
                    let stamp = match value.change {
                        TreatmentChange::Acceptance(value) => value.stamp,
                        TreatmentChange::HoldPlaced(value) => value.stamp,
                        TreatmentChange::HoldReleased { release, .. } => *release,
                    };
                    (Some(stamp.occurred), stamp.observation)
                }
            };
            (kind, occurred, observation)
        }
    };
    Ok(HubTraceHistoryEntry {
        operation: id,
        revision,
        kind,
        actor,
        recorded_at,
        occurred,
        observation,
        result,
    })
}
