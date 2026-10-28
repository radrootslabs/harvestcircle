use super::super::super::{reference, stock};
use super::*;
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error},
};
use harvestcircle_domain::hub::portions::PhysicalLineage;
use harvestcircle_domain::hub::treatment::{
    HUB_TREATMENT_RECORDS_PER_TRACE, TreatmentChange, TreatmentState,
};
use std::collections::BTreeMap;

pub(in crate::hub) async fn state_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    graph: &PhysicalLineage,
) -> Result<TreatmentState, SafeError> {
    let records = records_on(tx, hub, graph).await?;
    restore(graph, &records)
}

fn restore(
    graph: &PhysicalLineage,
    records: &[HubHandlingReceipt],
) -> Result<TreatmentState, SafeError> {
    let mut decisions = Vec::new();
    let mut holds = BTreeMap::new();
    let mut releases = Vec::new();
    for record in records {
        let HubHandlingResult::Treatment(result) = &record.result else {
            return Err(corrupt_storage());
        };
        match &result.change {
            TreatmentChange::Acceptance(value) => decisions.push(*value.clone()),
            TreatmentChange::HoldPlaced(value) => {
                if holds
                    .insert(*value.stamp.id.as_bytes(), *value.clone())
                    .is_some()
                {
                    return Err(corrupt_storage());
                }
            }
            TreatmentChange::HoldReleased { hold, release } => {
                releases.push((*hold, *release.clone()))
            }
        }
    }
    for (id, release) in releases {
        let held = holds.get_mut(id.as_bytes()).ok_or_else(corrupt_storage)?;
        if held.release.replace(release).is_some() {
            return Err(corrupt_storage());
        }
    }
    TreatmentState::restore(graph, decisions, holds.into_values().collect())
        .map_err(|_| corrupt_storage())
}

async fn records_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    graph: &PhysicalLineage,
) -> Result<Vec<HubHandlingReceipt>, SafeError> {
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_treatment_records WHERE trace_id=?")
            .bind(graph.trace().as_bytes().as_slice())
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
    if !(0..=(HUB_TREATMENT_RECORDS_PER_TRACE * 2) as i64).contains(&count) {
        return Err(corrupt_storage());
    }
    let ids:Vec<String>=sqlx::query_scalar("SELECT substr(t.operation_id,1,37) FROM hub_treatment_records t JOIN hub_stock_operations o ON o.operation_id=t.operation_id WHERE t.trace_id=? ORDER BY o.stock_revision LIMIT 513")
        .bind(graph.trace().as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if ids.len() != count as usize {
        return Err(corrupt_storage());
    }
    let mut records = Vec::with_capacity(ids.len());
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let operation = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        if operation.revision.value() > graph.revision().value() {
            return Err(corrupt_storage());
        }
        records.push(history::decode_on(tx, graph, operation).await?);
    }
    Ok(records)
}

fn check_versions(
    revision: HubRevision,
    catalog: HubRevision,
    expected: Option<HubRevision>,
    expected_catalog: Option<HubRevision>,
) -> Result<(), SafeError> {
    if expected.is_some_and(|value| value != revision)
        || expected_catalog.is_some_and(|value| value != catalog)
    {
        return Err(conflict());
    }
    Ok(())
}

impl Database {
    pub async fn hub_treatment(
        &self,
        actor: PublicKey,
        scope: HubTreatmentScope,
    ) -> Result<HubTreatmentSnapshot, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
                    let stock = stock::read::state_on(tx, hub.id()).await?;
                    check_versions(
                        stock.revision,
                        catalog.revision(),
                        scope.expected_revision,
                        scope.expected_catalog_revision,
                    )?;
                    let loaded = super::super::lineage::load_on(
                        tx,
                        hub.id(),
                        scope.trace,
                        &catalog,
                        stock.revision,
                    )
                    .await?;
                    let records = records_on(tx, hub.id(), &loaded.graph).await?;
                    let treatment = restore(&loaded.graph, &records)?;
                    let projection = treatment
                        .project(&loaded.graph, catalog.handling_basis())
                        .map_err(|_| corrupt_storage())?;
                    let operations = records
                        .iter()
                        .map(|record| {
                            let HubHandlingResult::Treatment(result) = &record.result else {
                                return Err(corrupt_storage());
                            };
                            let stamp = match &result.change {
                                TreatmentChange::Acceptance(value) => &value.stamp,
                                TreatmentChange::HoldPlaced(value) => &value.stamp,
                                TreatmentChange::HoldReleased { release, .. } => release,
                            };
                            Ok((*stamp.id.as_bytes(), record.operation.clone()))
                        })
                        .collect::<Result<BTreeMap<_, _>, SafeError>>()?;
                    let mut holds = Vec::with_capacity(treatment.holds().len());
                    for held in treatment.holds() {
                        let packs = loaded
                            .graph
                            .resolve(&held.scope)
                            .map_err(|_| corrupt_storage())?
                            .packs();
                        let operation = operations
                            .get(held.stamp.id.as_bytes())
                            .cloned()
                            .ok_or_else(corrupt_storage)?;
                        let released = held
                            .release
                            .as_ref()
                            .map(|release| {
                                operations
                                    .get(release.id.as_bytes())
                                    .cloned()
                                    .ok_or_else(corrupt_storage)
                            })
                            .transpose()?;
                        holds.push(HubHoldRow {
                            hold: held.clone(),
                            created_operation: operation,
                            released_operation: released,
                            current_packs: packs,
                        });
                    }
                    Ok(HubTreatmentSnapshot {
                        revision: stock.revision,
                        catalog_revision: catalog.revision(),
                        trace: loaded.trace,
                        projection,
                        normal_operations_available: capacity::available_on(tx, hub.id()).await?,
                        holds,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }

    pub async fn hub_treatment_history(
        &self,
        actor: PublicKey,
        query: HubTreatmentQuery,
    ) -> Result<HubTreatmentPage, SafeError> {
        query.validate()?;
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
                    let stock = stock::read::state_on(tx, hub.id()).await?;
                    check_versions(
                        stock.revision,
                        catalog.revision(),
                        query.expected_revision,
                        query.expected_catalog_revision,
                    )?;
                    let loaded = super::super::lineage::load_on(
                        tx,
                        hub.id(),
                        query.trace,
                        &catalog,
                        stock.revision,
                    )
                    .await?;
                    let records = records_on(tx, hub.id(), &loaded.graph).await?;
                    restore(&loaded.graph, &records)?;
                    let total = u32::try_from(records.len()).map_err(|_| corrupt_storage())?;
                    if query.offset > total {
                        return Err(invalid_input());
                    }
                    let rows = records
                        .into_iter()
                        .skip(query.offset as usize)
                        .take(query.limit as usize)
                        .collect::<Vec<_>>();
                    Ok(HubTreatmentPage {
                        revision: stock.revision,
                        catalog_revision: catalog.revision(),
                        offset: query.offset,
                        total,
                        more: query.offset
                            + u32::try_from(rows.len()).map_err(|_| corrupt_storage())?
                            < total,
                        basis: catalog.handling_basis(),
                        rows,
                    })
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
