//! Every source identity is backed by its first canonical population. No code/
//! issuer matching merges IDs; neither raw registry rows nor saved input suffice.
use super::*;
use harvestcircle_domain::hub::{
    reference::HUB_REFERENCE_HISTORY_CAPACITY,
    stock::{HUB_STOCK_EVENT_CAPACITY, HUB_TRACE_CAPACITY, StockPopulation, validate_source_reuse},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(in crate::hub::corrections) struct Population {
    pub operation: DurableRequestId,
    pub revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub recorded_at: UnixTimestamp,
    pub actor: PublicKey,
    pub population: StockPopulation,
}

pub(in crate::hub::corrections) async fn populations_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<Vec<Population>, SafeError> {
    // Start from original operations as well as present bodies: a later reuse
    // may disappear without altering the source registry's first creator.
    let missing:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_receipts r ON r.operation_id=o.operation_id ",
        "WHERE o.kind IN ('receipt','opening','return') AND r.operation_id IS NULL"
    )).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if missing != 0 {
        return Err(corrupt_storage());
    }
    // Only a validated count original determines whether posting created a
    // population. A missing surplus body must not look like a non-surplus post.
    let counts:Vec<String>=sqlx::query_scalar("SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE kind='count_post' ORDER BY stock_revision LIMIT ?")
        .bind(i64::try_from(HUB_STOCK_EVENT_CAPACITY+1).map_err(|_|corrupt_storage())?)
        .fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if counts.len() > HUB_STOCK_EVENT_CAPACITY {
        return Err(corrupt_storage());
    }
    for id in counts {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let operation = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        Box::pin(crate::hub::counts::posting::original_on(tx, hub, operation)).await?;
    }
    // Unfiltered scans prevent altered denormalized hub/trace fields from hiding
    // an unrelated original or identity from this complete native context.
    let rows=sqlx::query("SELECT substr(trace_id,1,17) AS trace_id,substr(operation_id,1,37) AS operation FROM hub_receipts ORDER BY trace_id LIMIT ?")
        .bind(i64::try_from(HUB_TRACE_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() > HUB_TRACE_CAPACITY {
        return Err(corrupt_storage());
    }
    let mut populations = Vec::with_capacity(rows.len());
    for row in rows {
        let trace = stock::read::record_id(&row, "trace_id")?;
        let operation = DurableRequestId::parse(
            row.try_get::<String, _>("operation")
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &operation)
            .await?
            .ok_or_else(corrupt_storage)?;
        let (revision, catalog_revision, recorded_at, actor) =
            (op.revision, op.catalog_revision, op.recorded_at, op.actor);
        let population = Box::pin(stock::read::population_on(tx, hub, op, trace)).await?;
        if population.id != trace {
            return Err(corrupt_storage());
        }
        populations.push(Population {
            operation,
            revision,
            catalog_revision,
            recorded_at,
            actor,
            population,
        });
    }
    Ok(populations)
}

pub(in crate::hub::corrections) async fn verify_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    populations: Vec<Population>,
    corrections: &[harvestcircle_domain::hub::corrections::SourceAttributionCorrection],
) -> Result<Vec<SourceAttribution>, SafeError> {
    let mut origins = populations
        .iter()
        .map(|p| {
            Ok((
                p.operation.clone(),
                p.revision,
                p.population.source.clone(),
                p.population.source_issuer.clone(),
            ))
        })
        .collect::<Result<Vec<_>, SafeError>>()?;
    for c in corrections {
        origins.push((
            crate::hub::actuals::common::operation(c.stamp.operation)?,
            c.stamp.stock_revision,
            c.after.source.clone(),
            c.after.issuer.clone(),
        ));
    }
    let mut associations: BTreeMap<HubRecordId, Vec<usize>> = BTreeMap::new();
    for (i, (_, _, source, _)) in origins.iter().enumerate() {
        if let SourceAttribution::Referenced { source, .. } = source {
            associations.entry(source.id).or_default().push(i);
        }
    }
    let rows=sqlx::query(concat!(
        "SELECT substr(source_id,1,17) AS source_id,substr(hub_id,1,17) AS hub_id,substr(created_operation_id,1,37) AS created,",
        "substr(issuer_id,1,17) AS issuer_id,issuer_revision,substr(kind,1,21) AS source_kind,",
        "substr(CAST(source_code AS BLOB),1,129) AS source_code,substr(CAST(provenance AS BLOB),1,513) AS provenance,",
        "'established' AS source_certainty FROM hub_source_references ORDER BY source_id LIMIT ?"
    )).bind(i64::try_from(HUB_REFERENCE_HISTORY_CAPACITY+1).map_err(|_|corrupt_storage())?).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if rows.len() > HUB_REFERENCE_HISTORY_CAPACITY || rows.len() != associations.len() {
        return Err(corrupt_storage());
    }
    let mut seen = BTreeSet::new();
    let mut sources = Vec::with_capacity(rows.len());
    for row in rows {
        let source_id = stock::read::record_id(&row, "source_id")?;
        let raw_hub: Vec<u8> = row.try_get("hub_id").map_err(|_| corrupt_storage())?;
        if raw_hub.as_slice() != hub.as_bytes().as_slice() || !seen.insert(source_id) {
            return Err(corrupt_storage());
        }
        let indices = associations.get(&source_id).ok_or_else(corrupt_storage)?;
        let first = indices
            .iter()
            .map(|i| &origins[*i])
            .min_by_key(|p| p.1.value())
            .ok_or_else(corrupt_storage)?;
        if row
            .try_get::<String, _>("created")
            .map_err(|_| corrupt_storage())?
            != first.0.as_str()
        {
            return Err(corrupt_storage());
        }
        let SourceAttribution::Referenced { source: raw, .. } = stock::read::source_from_row(&row)?
        else {
            return Err(corrupt_storage());
        };
        let SourceAttribution::Referenced {
            source: canonical, ..
        } = &first.2
        else {
            return Err(corrupt_storage());
        };
        let issuer_revision: Option<i64> = row
            .try_get("issuer_revision")
            .map_err(|_| corrupt_storage())?;
        let expected = first
            .3
            .as_ref()
            .map(|r| i64::try_from(r.revision.value()).map_err(|_| corrupt_storage()))
            .transpose()?;
        if raw != *canonical || issuer_revision != expected {
            return Err(corrupt_storage());
        }
        for index in indices {
            validate_source_reuse(&origins[*index].2, [first.2.clone()])
                .map_err(|_| corrupt_storage())?;
        }
        sources.push(first.2.clone());
    }
    Ok(sources)
}
