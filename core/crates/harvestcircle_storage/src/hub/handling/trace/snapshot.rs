use super::*;
use harvestcircle_domain::hub::portions::HUB_LINEAGE_GROUPS;
use harvestcircle_domain::hub::trace::{TraceDeparture, TraceExit, TracePosition};
use std::collections::BTreeMap;

impl Database {
    pub async fn hub_trace_snapshot(
        &self,
        actor: PublicKey,
        query: HubTraceSnapshotQuery,
    ) -> Result<TraceSnapshot, SafeError> {
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
                    load_on(tx, hub, &state, &catalog, query.trace).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    state: &stock::read::StockState,
    catalog: &ReferenceCatalog,
    trace: HubRecordId,
) -> Result<TraceSnapshot, SafeError> {
    let loaded = super::super::lineage::load_on(tx, hub, trace, catalog, state.revision).await?;
    let treatment = super::super::treatment::read::state_on(tx, hub, &loaded.graph).await?;
    let projection = treatment
        .project(&loaded.graph, catalog.handling_basis())
        .map_err(|_| corrupt_storage())?;
    let mut treatments = projection
        .groups
        .into_iter()
        .map(|g| (g.group, g))
        .collect::<BTreeMap<_, _>>();
    let mut on_hand = Vec::new();
    for row in &loaded.rows {
        if row.packs.value() == 0 {
            continue;
        }
        on_hand.push(TracePosition {
            group: row.id,
            revision: row.revision,
            packs: row.packs,
            location: row.location.clone().ok_or_else(corrupt_storage)?,
            identification: row.identification.clone(),
            treatment: treatments.remove(&row.id).ok_or_else(corrupt_storage)?,
        });
    }
    if !treatments.is_empty() {
        return Err(corrupt_storage());
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT operation_id) FROM hub_departure_lines WHERE trace_id=?",
    )
    .bind(trace.as_bytes().as_slice())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if !(0..=HUB_LINEAGE_GROUPS as i64).contains(&count) {
        return Err(corrupt_storage());
    }
    let operations:Vec<String>=sqlx::query_scalar("SELECT DISTINCT substr(operation_id,1,37) FROM hub_departure_lines WHERE trace_id=? ORDER BY operation_id LIMIT 257")
        .bind(trace.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if operations.len() != count as usize {
        return Err(corrupt_storage());
    }
    let mut exits = Vec::new();
    for operation in operations {
        let operation = DurableRequestId::parse(operation).map_err(|_| corrupt_storage())?;
        let operation = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        let original = super::super::departure::read::original_on(tx, hub, operation).await?;
        let original = &original.record;
        let issued = original.paper.versions.last().ok_or_else(corrupt_storage)?;
        let departure = TraceDeparture {
            id: original.id,
            operation: original.operation,
            stock_revision: original.stock_revision,
            preparation: original.paper.id,
            identification: issued.id,
            recipient: issued.material.recipient.clone(),
            actor: original.actor,
            recorded_at: original.recorded_at,
            occurred: original.occurred,
            observation: original.observation.clone(),
        };
        let mut found = false;
        for consumed in original.consumed.iter().filter(|c| c.before.trace == trace) {
            found = true;
            let current = loaded
                .rows
                .iter()
                .find(|row| row.id == consumed.before.id)
                .ok_or_else(corrupt_storage)?;
            if current.packs.value() != 0 || current.revision != consumed.after_revision {
                return Err(corrupt_storage());
            }
            exits.push(TraceExit {
                group: consumed.before.id,
                packs: consumed.before.packs,
                departure: departure.clone(),
            });
        }
        if !found || exits.len() > HUB_LINEAGE_GROUPS {
            return Err(corrupt_storage());
        }
    }
    exits.sort_by_key(|exit| (exit.departure.stock_revision.value(), exit.group));
    let snapshot = TraceSnapshot {
        hub,
        stock_revision: state.revision,
        catalog_revision: catalog.revision(),
        trace: loaded.trace,
        on_hand,
        exits,
    };
    snapshot.balance().map_err(|_| corrupt_storage())?;
    Ok(snapshot)
}
